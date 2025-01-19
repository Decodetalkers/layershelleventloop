mod state;

use std::{borrow::Cow, mem::ManuallyDrop, os::fd::AsFd, sync::Arc, time::Duration};

use crate::{
    actions::{LayerShellAction, LayerShellActionVec, LayershellCustomActions},
    clipboard::LayerShellClipboard,
    conversion,
    error::Error,
    settings::VirtualKeyboardSettings,
};

use super::{Appearance, DefaultStyle};
use iced_graphics::{compositor, Compositor};
use state::State;

use iced_core::{time::Instant, window as IcedCoreWindow, Event as IcedCoreEvent, Size};

use iced_runtime::{task::Task, user_interface, Action, Debug, Program, UserInterface};

use iced_futures::{Executor, Runtime, Subscription};

use layershellev::{
    calloop::timer::{TimeoutAction, Timer},
    reexport::wayland_client::{WlCompositor, WlRegion},
    reexport::zwp_virtual_keyboard_v1,
    LayerEvent, ReturnData, StartMode, WindowWrapper,
};

use futures::{channel::mpsc, StreamExt};

use crate::{actions::ActionCallback, event::IcedLayerEvent, proxy::IcedProxy, settings::Settings};

/// An interactive, native cross-platform application.
///
/// This trait is the main entrypoint of Iced. Once implemented, you can run
/// your GUI application by simply calling [`run`]. It will run in
/// its own window.
///
/// An [`Application`] can execute asynchronous actions by returning a
/// [`Task`] in some of its methods.
///
/// When using an [`Application`] with the `debug` feature enabled, a debug view
/// can be toggled by pressing `F12`.
pub trait Application: Program
where
    Self::Theme: DefaultStyle,
{
    /// The data needed to initialize your [`Application`].
    type Flags;

    /// Initializes the [`Application`] with the flags provided to
    /// [`run`] as part of the [`Settings`].
    ///
    /// Here is where you should return the initial state of your app.
    ///
    /// Additionally, you can return a [`Task`] if you need to perform some
    /// async action in the background on startup. This is useful if you want to
    /// load state from a file, perform an initial HTTP request, etc.
    fn new(flags: Self::Flags) -> (Self, Task<Self::Message>);

    fn namespace(&self) -> String;
    /// Returns the current title of the [`Application`].
    ///
    /// This title can be dynamic! The runtime will automatically update the
    /// title of your application when necessary.
    fn title(&self) -> String {
        self.namespace()
    }

    /// Returns the current `Theme` of the [`Application`].
    fn theme(&self) -> Self::Theme;

    /// Returns the `Style` variation of the `Theme`.
    fn style(&self, theme: &Self::Theme) -> Appearance {
        theme.default_style()
    }

    /// Returns the event `Subscription` for the current state of the
    /// application.
    ///
    /// The messages produced by the `Subscription` will be handled by
    /// [`update`](#tymethod.update).
    ///
    /// A `Subscription` will be kept alive as long as you keep returning it!
    ///
    /// By default, it returns an empty subscription.
    fn subscription(&self) -> Subscription<Self::Message> {
        Subscription::none()
    }

    /// Returns the scale factor of the [`Application`].
    ///
    /// It can be used to dynamically control the size of the UI at runtime
    /// (i.e. zooming).
    ///
    /// For instance, a scale factor of `2.0` will make widgets twice as big,
    /// while a scale factor of `0.5` will shrink them to half their size.
    ///
    /// By default, it returns `1.0`.
    fn scale_factor(&self) -> f64 {
        1.0
    }

    /// Defines whether or not to use natural scrolling
    fn natural_scroll(&self) -> bool {
        false
    }

    /// Returns whether the [`Application`] should be terminated.
    ///
    /// By default, it returns `false`.
    fn should_exit(&self) -> bool {
        false
    }
}

type SingleRuntime<E, Message> = Runtime<E, IcedProxy<Action<Message>>, Action<Message>>;

// a dispatch loop, another is listen loop
pub fn run<A, E, C>(
    settings: Settings<A::Flags>,
    compositor_settings: iced_graphics::Settings,
) -> Result<(), Error>
where
    A: Application + 'static,
    E: Executor + 'static,
    C: Compositor<Renderer = A::Renderer> + 'static,
    A::Theme: DefaultStyle,
    A::Message: 'static + TryInto<LayershellCustomActions, Error = A::Message>,
{
    use futures::task;
    use futures::Future;

    let mut debug = Debug::new();
    debug.startup_started();

    let (message_sender, message_receiver) = std::sync::mpsc::channel::<Action<A::Message>>();

    let proxy = IcedProxy::new(message_sender);
    let mut runtime: SingleRuntime<E, A::Message> = {
        let executor = E::new().map_err(Error::ExecutorCreationFailed)?;

        Runtime::new(executor, proxy)
    };

    let (application, task) = {
        let flags = settings.flags;

        runtime.enter(|| A::new(flags))
    };

    assert!(!matches!(
        settings.layer_settings.start_mode,
        StartMode::AllScreens | StartMode::Background
    ));

    let ev = layershellev::WindowStateSimple::new(&application.namespace())
        .with_use_display_handle(true)
        .with_option_size(settings.layer_settings.size)
        .with_layer(settings.layer_settings.layer)
        .with_events_transparent(settings.layer_settings.events_transparent)
        .with_anchor(settings.layer_settings.anchor)
        .with_exclusize_zone(settings.layer_settings.exclusive_zone)
        .with_margin(settings.layer_settings.margin)
        .with_keyboard_interacivity(settings.layer_settings.keyboard_interactivity)
        .with_start_mode(settings.layer_settings.start_mode)
        .build()
        .expect("Cannot create layershell");

    let window = Arc::new(ev.gen_main_wrapper());

    if let Some(stream) = iced_runtime::task::into_stream(task) {
        runtime.run(stream);
    }

    runtime.track(iced_futures::subscription::into_recipes(
        runtime.enter(|| application.subscription().map(Action::Output)),
    ));

    let state = State::new(&application, &ev);

    let (mut event_sender, event_receiver) =
        mpsc::unbounded::<IcedLayerEvent<Action<A::Message>>>();
    let (control_sender, mut control_receiver) = mpsc::unbounded::<LayerShellActionVec>();

    let mut instance = Box::pin(run_instance::<A, E, C>(
        application,
        compositor_settings,
        runtime,
        debug,
        event_receiver,
        control_sender,
        state,
        window,
        settings.fonts,
    ));

    let mut context = task::Context::from_waker(task::noop_waker_ref());
    let mut wl_input_region: Option<WlRegion> = None;
    let mut pointer_serial: u32 = 0;

    let _ = ev.running_with_proxy(message_receiver, move |event, ev, _| {
        use layershellev::DispatchMessage;
        let mut def_returndata = ReturnData::None;
        match event {
            LayerEvent::InitRequest => {
                def_returndata = ReturnData::RequestBind;
            }
            LayerEvent::BindProvide(globals, qh) => {
                let wl_compositor = globals
                    .bind::<WlCompositor, _, _>(qh, 1..=1, ())
                    .expect("could not bind wl_compositor");
                wl_input_region = Some(wl_compositor.create_region(qh, ()));

                if settings.virtual_keyboard_support.is_some() {
                    let virtual_keyboard_manager = globals
                        .bind::<zwp_virtual_keyboard_v1::ZwpVirtualKeyboardManagerV1, _, _>(
                            qh,
                            1..=1,
                            (),
                        )
                        .expect("no support virtual_keyboard");
                    let VirtualKeyboardSettings {
                        file,
                        keymap_size,
                        keymap_format,
                    } = settings.virtual_keyboard_support.as_ref().unwrap();
                    let seat = ev.get_seat();
                    let virtual_keyboard_in =
                        virtual_keyboard_manager.create_virtual_keyboard(seat, qh, ());
                    virtual_keyboard_in.keymap((*keymap_format).into(), file.as_fd(), *keymap_size);
                    ev.set_virtual_keyboard(virtual_keyboard_in);
                }
            }
            LayerEvent::RequestMessages(message) => {
                if let DispatchMessage::MouseEnter { serial, .. } = message {
                    pointer_serial = *serial;
                }

                event_sender
                    .start_send(message.into())
                    .expect("Cannot send");
            }
            LayerEvent::NormalDispatch => {
                event_sender
                    .start_send(IcedLayerEvent::NormalUpdate)
                    .expect("Cannot send");
            }
            LayerEvent::UserEvent(event) => {
                event_sender
                    .start_send(IcedLayerEvent::UserEvent(event))
                    .ok();
            }
            _ => {}
        }
        let poll = instance.as_mut().poll(&mut context);

        let task::Poll::Pending = poll else {
            return ReturnData::RequestExit;
        };

        let Ok(Some(flows)) = control_receiver.try_next() else {
            return def_returndata;
        };
        for flow in flows {
            match flow {
                LayerShellAction::CustomActions(action) => match action {
                    LayershellCustomActions::AnchorChange(anchor) => {
                        ev.main_window().set_anchor(anchor);
                    }
                    LayershellCustomActions::AnchorSizeChange(anchor, size) => {
                        ev.main_window().set_anchor_with_size(anchor, size);
                    }
                    LayershellCustomActions::LayerChange(layer) => {
                        ev.main_window().set_layer(layer);
                    }
                    LayershellCustomActions::SetInputRegion(ActionCallback(set_region)) => {
                        let window = ev.main_window();

                        let region = wl_input_region.as_ref().expect("region not found");
                        let window_size = window.get_size();
                        let width: i32 = window_size.0.try_into().unwrap_or_default();
                        let height: i32 = window_size.1.try_into().unwrap_or_default();

                        region.subtract(0, 0, width, height);
                        set_region(region);

                        window.get_wlsurface().set_input_region(Some(region));
                    }
                    LayershellCustomActions::MarginChange(margin) => {
                        ev.main_window().set_margin(margin);
                    }
                    LayershellCustomActions::SizeChange((width, height)) => {
                        ev.main_window().set_size((width, height));
                    }
                    LayershellCustomActions::VirtualKeyboardPressed { time, key } => {
                        use layershellev::reexport::wayland_client::KeyState;
                        let ky = ev.get_virtual_keyboard().unwrap();
                        ky.key(time, key, KeyState::Pressed.into());

                        let eh = ev.get_loop_handler().unwrap();
                        eh.insert_source(
                            Timer::from_duration(Duration::from_micros(100)),
                            move |_, _, state| {
                                let ky = state.get_virtual_keyboard().unwrap();

                                ky.key(time, key, KeyState::Released.into());
                                TimeoutAction::Drop
                            },
                        )
                        .ok();
                    }
                    _ => {}
                },
                LayerShellAction::Mouse(mouse) => {
                    let Some(pointer) = ev.get_pointer() else {
                        return ReturnData::None;
                    };

                    ev.append_return_data(ReturnData::RequestSetCursorShape((
                        conversion::mouse_interaction(mouse),
                        pointer.clone(),
                        pointer_serial,
                    )));
                }
                LayerShellAction::RedrawAll => {
                    ev.append_return_data(ReturnData::RedrawAllRequest);
                }
                LayerShellAction::RedrawWindow(index) => {
                    ev.append_return_data(ReturnData::RedrawIndexRequest(index));
                }
                _ => {}
            }
        }
        def_returndata
    });
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn run_instance<A, E, C>(
    mut application: A,
    compositor_settings: iced_graphics::Settings,
    mut runtime: SingleRuntime<E, A::Message>,
    mut debug: Debug,
    mut event_receiver: mpsc::UnboundedReceiver<IcedLayerEvent<Action<A::Message>>>,
    mut control_sender: mpsc::UnboundedSender<LayerShellActionVec>,
    mut state: State<A>,
    window: Arc<WindowWrapper>,
    fonts: Vec<Cow<'static, [u8]>>,
) where
    A: Application + 'static,
    E: Executor + 'static,
    C: Compositor<Renderer = A::Renderer> + 'static,
    A::Theme: DefaultStyle,
    A::Message: 'static + TryInto<LayershellCustomActions, Error = A::Message>,
{
    use iced_core::mouse;
    use iced_core::Event;

    let mut compositor = C::new(compositor_settings, window.clone())
        .await
        .expect("Cannot create compositor");
    for font in fonts {
        compositor.load_font(font);
    }

    let mut renderer = compositor.create_renderer();

    let cache = user_interface::Cache::default();

    // HACK: the surface size should not be set as 0, 0
    // but it will changed later
    // so here set it to 1, 1
    let mut surface = compositor.create_surface(window.clone(), 1, 1);

    let mut should_exit = false;

    let mut clipboard = LayerShellClipboard::connect(&window);

    let mut mouse_interaction = mouse::Interaction::default();
    let mut messages = Vec::new();
    let mut events: Vec<Event> = Vec::new();
    let mut custom_actions = Vec::new();

    let mut user_interface = ManuallyDrop::new(build_user_interface(
        &application,
        cache,
        &mut renderer,
        state.logical_size(),
        &mut debug,
    ));

    debug.startup_finished();

    let main_id = IcedCoreWindow::Id::unique();

    while let Some(event) = event_receiver.next().await {
        match event {
            IcedLayerEvent::RequestRefresh {
                width,
                height,
                fractal_scale,
            } => {
                state.update_view_port(width, height, fractal_scale);
                let logical_size = state.logical_size();

                debug.layout_started();
                user_interface = ManuallyDrop::new(
                    ManuallyDrop::into_inner(user_interface).relayout(logical_size, &mut renderer),
                );
                debug.layout_finished();

                let physical_size = state.physical_size();
                compositor.configure_surface(
                    &mut surface,
                    physical_size.width,
                    physical_size.height,
                );
                let redraw_event =
                    IcedCoreEvent::Window(IcedCoreWindow::Event::RedrawRequested(Instant::now()));

                user_interface.update(
                    &[redraw_event.clone()],
                    state.cursor(),
                    &mut renderer,
                    &mut clipboard,
                    &mut messages,
                );
                events.push(redraw_event.clone());
                runtime.broadcast(iced_futures::subscription::Event::Interaction {
                    window: main_id,
                    event: redraw_event,
                    status: iced_core::event::Status::Ignored,
                });

                debug.draw_started();
                let new_mouse_interaction = user_interface.draw(
                    &mut renderer,
                    state.theme(),
                    &iced_core::renderer::Style {
                        text_color: state.text_color(),
                    },
                    state.cursor(),
                );
                debug.draw_finished();

                if new_mouse_interaction != mouse_interaction {
                    custom_actions.push(LayerShellAction::Mouse(new_mouse_interaction));
                    mouse_interaction = new_mouse_interaction;
                }
                // TODO: check mouse_interaction

                debug.render_started();

                debug.draw_started();
                user_interface.draw(
                    &mut renderer,
                    &application.theme(),
                    &iced_core::renderer::Style {
                        text_color: state.text_color(),
                    },
                    state.cursor(),
                );
                debug.draw_finished();
                compositor
                    .present(
                        &mut renderer,
                        &mut surface,
                        state.viewport(),
                        state.background_color(),
                        &debug.overlay(),
                    )
                    .ok();
                match compositor.present(
                    &mut renderer,
                    &mut surface,
                    state.viewport(),
                    state.background_color(),
                    &debug.overlay(),
                ) {
                    Ok(()) => {
                        debug.render_finished();
                    }
                    Err(error) => match error {
                        compositor::SurfaceError::OutOfMemory => {
                            panic!("{:?}", error);
                        }
                        _ => {
                            debug.render_finished();
                            tracing::error!(
                                "Error {error:?} when \
                                        presenting surface."
                            );
                        }
                    },
                }
            }
            IcedLayerEvent::Window(event) => {
                state.update(&event);

                if let Some(event) = conversion::window_event(&event, state.modifiers()) {
                    events.push(event);
                }
            }
            IcedLayerEvent::UserEvent(event) => {
                let mut cache = ManuallyDrop::into_inner(user_interface).into_cache();
                run_action(
                    &application,
                    &mut compositor,
                    &mut surface,
                    &mut cache,
                    &state,
                    &mut renderer,
                    event,
                    &mut messages,
                    &mut clipboard,
                    &mut custom_actions,
                    &mut should_exit,
                    &mut debug,
                );
                user_interface = ManuallyDrop::new(build_user_interface(
                    &application,
                    cache,
                    &mut renderer,
                    state.logical_size(),
                    &mut debug,
                ));
                if should_exit {
                    break;
                }
            }
            IcedLayerEvent::NormalUpdate => {
                if events.is_empty() && messages.is_empty() {
                    continue;
                }
                debug.event_processing_started();
                let (interface_state, statuses) = user_interface.update(
                    &events,
                    state.cursor(),
                    &mut renderer,
                    &mut clipboard,
                    &mut messages,
                );
                debug.event_processing_finished();

                for (event, status) in events.drain(..).zip(statuses.into_iter()) {
                    runtime.broadcast(iced_futures::subscription::Event::Interaction {
                        window: main_id,
                        event,
                        status,
                    });
                }

                if !messages.is_empty()
                    || matches!(interface_state, user_interface::State::Outdated)
                {
                    let cache = ManuallyDrop::into_inner(user_interface).into_cache();
                    // Update application
                    update(
                        &mut application,
                        &mut state,
                        &mut runtime,
                        &mut debug,
                        &mut messages,
                    );
                    user_interface = ManuallyDrop::new(build_user_interface(
                        &application,
                        cache,
                        &mut renderer,
                        state.logical_size(),
                        &mut debug,
                    ));
                }
                custom_actions.push(LayerShellAction::RedrawAll);
            }
            _ => unreachable!(),
        }
        let mut copyactions = vec![];
        std::mem::swap(&mut copyactions, &mut custom_actions);
        control_sender.start_send(copyactions).ok();
    }

    drop(ManuallyDrop::into_inner(user_interface));
}

/// Builds a [`UserInterface`] for the provided [`Application`], logging
/// [`struct@Debug`] information accordingly.
pub fn build_user_interface<'a, A: Application>(
    application: &'a A,
    cache: user_interface::Cache,
    renderer: &mut A::Renderer,
    size: Size,
    debug: &mut Debug,
) -> UserInterface<'a, A::Message, A::Theme, A::Renderer>
where
    A::Theme: DefaultStyle,
{
    debug.view_started();
    let view = application.view();
    debug.view_finished();

    debug.layout_started();
    let user_interface = UserInterface::build(view, size, cache, renderer);
    debug.layout_finished();
    user_interface
}

/// Updates an [`Application`] by feeding it the provided messages, spawning any
/// tracking its [`Subscription`].
#[allow(clippy::too_many_arguments)]
pub(crate) fn update<A: Application, E: Executor>(
    application: &mut A,
    state: &mut State<A>,
    runtime: &mut SingleRuntime<E, A::Message>,
    debug: &mut Debug,
    messages: &mut Vec<A::Message>,
) where
    A::Theme: DefaultStyle,
    A::Message: 'static,
{
    for message in messages.drain(..) {
        debug.log_message(&message);

        debug.update_started();
        let task = runtime.enter(|| application.update(message));
        debug.update_finished();

        if let Some(stream) = iced_runtime::task::into_stream(task) {
            runtime.run(stream);
        }
    }
    state.synchronize(application);

    let subscription = runtime.enter(|| application.subscription());
    runtime.track(iced_futures::subscription::into_recipes(
        subscription.map(Action::Output),
    ));
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn run_action<A, C>(
    application: &A,
    compositor: &mut C,
    surface: &mut C::Surface,
    cache: &mut user_interface::Cache,
    state: &State<A>,
    renderer: &mut A::Renderer,
    event: Action<A::Message>,
    messages: &mut Vec<A::Message>,
    clipboard: &mut LayerShellClipboard,
    custom_actions: &mut Vec<LayerShellAction>,
    should_exit: &mut bool,
    debug: &mut Debug,
) where
    A: Application,
    C: Compositor<Renderer = A::Renderer> + 'static,
    A::Theme: DefaultStyle,
    A::Message: 'static + TryInto<LayershellCustomActions, Error = A::Message>,
{
    use iced_core::widget::operation;
    use iced_runtime::clipboard;
    use iced_runtime::window;
    use iced_runtime::window::Action as WinowAction;
    use iced_runtime::Action;
    match event {
        Action::Output(stream) => match stream.try_into() {
            Ok(action) => custom_actions.push(LayerShellAction::CustomActions(action)),
            Err(stream) => {
                messages.push(stream);
            }
        },

        Action::Clipboard(action) => match action {
            clipboard::Action::Read { target, channel } => {
                let _ = channel.send(clipboard.read(target));
            }
            clipboard::Action::Write { target, contents } => {
                clipboard.write(target, contents);
            }
        },
        Action::Widget(action) => {
            let mut current_cache = std::mem::take(cache);
            let mut current_operation = Some(action);

            let mut user_interface = build_user_interface(
                application,
                current_cache,
                renderer,
                state.logical_size(),
                debug,
            );

            while let Some(mut operation) = current_operation.take() {
                user_interface.operate(renderer, operation.as_mut());

                match operation.finish() {
                    operation::Outcome::None => {}
                    operation::Outcome::Some(_message) => {
                        // TODO:
                    }
                    operation::Outcome::Chain(next) => {
                        current_operation = Some(next);
                    }
                }
            }

            current_cache = user_interface.into_cache();
            *cache = current_cache;
        }
        Action::Window(action) => match action {
            WinowAction::Close(_) => {
                *should_exit = true;
            }
            WinowAction::Screenshot(_id, channel) => {
                let bytes = compositor.screenshot(
                    renderer,
                    surface,
                    state.viewport(),
                    state.background_color(),
                    &debug.overlay(),
                );
                let _ = channel.send(window::Screenshot::new(
                    bytes,
                    state.physical_size(),
                    state.scale_factor(),
                ));
            }
            _ => {}
        },
        Action::Exit => {
            *should_exit = true;
        }
        Action::LoadFont { bytes, channel } => {
            // TODO: Error handling (?)
            compositor.load_font(bytes.clone());

            let _ = channel.send(Ok(()));
        }
        _ => {}
    }
}
