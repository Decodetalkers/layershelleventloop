mod state;
use crate::{
    actions::{SessionShellActionVec, UnLockAction},
    multi_window::window_manager::WindowManager,
};
use std::{borrow::Cow, collections::HashMap, f64, mem::ManuallyDrop, sync::Arc};

use crate::{
    actions::SessionShellAction, clipboard::SessionLockClipboard, conversion, error::Error,
};

use super::{Appearance, DefaultStyle};
use iced::Task;
use iced_graphics::Compositor;

use iced_core::{time::Instant, Size};

use iced_runtime::{multi_window::Program, user_interface, Action, Debug, UserInterface};

use iced_futures::{Executor, Runtime, Subscription};

use sessionlockev::{ReturnData, SessionLockEvent, WindowState, WindowWrapper};

use futures::{channel::mpsc, StreamExt};

use crate::{
    event::{IcedSessionLockEvent, MultiWindowIcedSessionLockEvent},
    proxy::IcedProxy,
    settings::Settings,
};

mod window_manager;

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

    /// Returns the current [`Program::Theme`] of the [`Application`].
    fn theme(&self) -> Self::Theme;

    /// Returns the `Style` variation of the `Theme`.
    fn style(&self, theme: &Self::Theme) -> Appearance {
        theme.default_style()
    }

    /// Returns the event `Subscription` for the current state of the
    /// application.
    ///
    /// The messages produced by the `Subscription` will be handled by
    /// `update`(#tymethod.update).
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
    fn scale_factor(&self, _window: iced::window::Id) -> f64 {
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

type SessionRuntime<E, Message> = Runtime<E, IcedProxy<Action<Message>>, Action<Message>>;

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
    A::Message: 'static + TryInto<UnLockAction, Error = A::Message>,
{
    use futures::task;
    use futures::Future;

    let mut debug = Debug::new();
    debug.startup_started();

    let (message_sender, message_receiver) = std::sync::mpsc::channel::<Action<A::Message>>();

    let proxy = IcedProxy::new(message_sender);
    let mut runtime: SessionRuntime<E, A::Message> = {
        let executor = E::new().map_err(Error::ExecutorCreationFailed)?;

        Runtime::new(executor, proxy)
    };
    let (application, task) = {
        let flags = settings.flags;

        runtime.enter(|| A::new(flags))
    };

    if let Some(stream) = iced_runtime::task::into_stream(task) {
        runtime.run(stream);
    }

    runtime.track(iced_futures::subscription::into_recipes(
        runtime.enter(|| application.subscription().map(Action::Output)),
    ));

    let ev: WindowState<()> = sessionlockev::WindowState::new()
        .with_use_display_handle(true)
        .build()
        .expect("Seems sessionlock is not supported");

    let window = Arc::new(ev.gen_main_wrapper());

    let (mut event_sender, event_receiver) =
        mpsc::unbounded::<MultiWindowIcedSessionLockEvent<Action<A::Message>>>();
    let (control_sender, mut control_receiver) = mpsc::unbounded::<SessionShellActionVec>();

    let mut instance = Box::pin(run_instance::<A, E, C>(
        application,
        compositor_settings,
        runtime,
        debug,
        event_receiver,
        control_sender,
        //state,
        window,
        settings.fonts,
    ));

    let mut context = task::Context::from_waker(task::noop_waker_ref());

    let mut pointer_serial: u32 = 0;

    let _ = ev.running_with_proxy(message_receiver, move |event, ev, id| {
        use sessionlockev::DispatchMessage;
        match event {
            SessionLockEvent::InitRequest => {}
            // TODO: maybe use it later
            SessionLockEvent::BindProvide(_, _) => {}
            SessionLockEvent::RequestMessages(message) => 'outside: {
                match message {
                    DispatchMessage::RequestRefresh {
                        width,
                        height,
                        scale_float,
                    } => {
                        event_sender
                            .start_send(MultiWindowIcedSessionLockEvent(
                                id,
                                IcedSessionLockEvent::RequestRefreshWithWrapper {
                                    width: *width,
                                    height: *height,
                                    scale_float: *scale_float,
                                    wrapper: ev
                                        .get_unit_with_id(id.unwrap())
                                        .unwrap()
                                        .gen_wrapper(),
                                },
                            ))
                            .expect("Cannot send");
                        break 'outside;
                    }
                    DispatchMessage::MouseEnter { serial, .. } => {
                        pointer_serial = *serial;
                    }
                    _ => {}
                }

                event_sender
                    .start_send(MultiWindowIcedSessionLockEvent(id, message.into()))
                    .expect("Cannot send");
            }
            SessionLockEvent::NormalDispatch => {
                event_sender
                    .start_send(MultiWindowIcedSessionLockEvent(
                        id,
                        IcedSessionLockEvent::NormalUpdate,
                    ))
                    .expect("Cannot send");
            }
            SessionLockEvent::UserEvent(event) => {
                event_sender
                    .start_send(MultiWindowIcedSessionLockEvent(
                        id,
                        IcedSessionLockEvent::UserEvent(event),
                    ))
                    .ok();
            }
            _ => {}
        }
        let poll = instance.as_mut().poll(&mut context);
        match poll {
            task::Poll::Pending => 'peddingBlock: {
                if let Some(flow) = control_receiver
                    .try_next()
                    .ok()
                    .flatten()
                    .and_then(|flows| flows.into_iter().next())
                {
                    match flow {
                        SessionShellAction::Mouse(mouse) => {
                            let Some(pointer) = ev.get_pointer() else {
                                break 'peddingBlock ReturnData::None;
                            };

                            break 'peddingBlock ReturnData::RequestSetCursorShape((
                                conversion::mouse_interaction(mouse),
                                pointer.clone(),
                                pointer_serial,
                            ));
                        }
                        SessionShellAction::RedrawAll => {
                            break 'peddingBlock ReturnData::RedrawAllRequest;
                        }
                        SessionShellAction::RedrawWindow(index) => {
                            break 'peddingBlock ReturnData::RedrawIndexRequest(index);
                        }
                    }
                }
                ReturnData::None
            }
            task::Poll::Ready(_) => ReturnData::RequestUnlockAndExist,
        }
    });
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn run_instance<A, E, C>(
    mut application: A,
    compositor_settings: iced_graphics::Settings,
    mut runtime: SessionRuntime<E, A::Message>,
    mut debug: Debug,
    mut event_receiver: mpsc::UnboundedReceiver<
        MultiWindowIcedSessionLockEvent<Action<A::Message>>,
    >,
    mut control_sender: mpsc::UnboundedSender<SessionShellActionVec>,
    window: Arc<WindowWrapper>,
    fonts: Vec<Cow<'static, [u8]>>,
) where
    A: Application + 'static,
    E: Executor + 'static,
    C: Compositor<Renderer = A::Renderer> + 'static,
    A::Theme: DefaultStyle,
    A::Message: 'static + TryInto<UnLockAction, Error = A::Message>,
{
    use iced::window;
    use iced_core::Event;
    let mut compositor = C::new(compositor_settings, window.clone())
        .await
        .expect("Cannot create compositer");
    for font in fonts {
        compositor.load_font(font);
    }
    let mut window_manager = WindowManager::new();

    let mut clipboard = SessionLockClipboard::connect(&window);
    let mut ui_caches: HashMap<window::Id, user_interface::Cache> = HashMap::new();

    let mut user_interfaces = ManuallyDrop::new(build_user_interfaces(
        &application,
        &mut debug,
        &mut window_manager,
        HashMap::new(),
    ));
    let mut events = Vec::new();
    let mut custom_actions = Vec::new();

    let mut should_exit = false;
    let mut messages = Vec::new();

    while let Some(event) = event_receiver.next().await {
        match event {
            MultiWindowIcedSessionLockEvent(
                _id,
                IcedSessionLockEvent::RequestRefreshWithWrapper {
                    width,
                    height,
                    wrapper,
                    scale_float,
                },
            ) => {
                let (id, window) = if window_manager.get_mut_alias(wrapper.id()).is_none() {
                    let id = window::Id::unique();

                    let window = window_manager.insert(
                        id,
                        (width, height),
                        scale_float,
                        Arc::new(wrapper),
                        &application,
                        &mut compositor,
                    );
                    let logical_size = window.state.logical_size();

                    let _ = user_interfaces.insert(
                        id,
                        build_user_interface(
                            &application,
                            user_interface::Cache::default(),
                            &mut window.renderer,
                            logical_size,
                            &mut debug,
                            id,
                        ),
                    );
                    let _ = ui_caches.insert(id, user_interface::Cache::default());

                    events.push((
                        Some(id),
                        Event::Window(window::Event::Opened {
                            position: None,
                            size: window.state.logical_size(),
                        }),
                    ));
                    (id, window)
                } else {
                    let (id, window) = window_manager.get_mut_alias(wrapper.id()).unwrap();
                    let ui = user_interfaces.remove(&id).expect("Get User interface");
                    window.state.update_view_port(width, height, scale_float);
                    let _ = user_interfaces.insert(
                        id,
                        ui.relayout(window.state.logical_size(), &mut window.renderer),
                    );
                    (id, window)
                };

                let ui = user_interfaces.get_mut(&id).expect("Get User interface");

                let redraw_event =
                    iced_core::Event::Window(window::Event::RedrawRequested(Instant::now()));
                let cursor = window.state.cursor();

                events.push((Some(id), redraw_event.clone()));
                ui.update(
                    &[redraw_event.clone()],
                    cursor,
                    &mut window.renderer,
                    &mut clipboard,
                    &mut messages,
                );

                debug.draw_started();
                let new_mouse_interaction = ui.draw(
                    &mut window.renderer,
                    window.state.theme(),
                    &iced_core::renderer::Style {
                        text_color: window.state.text_color(),
                    },
                    cursor,
                );
                debug.draw_finished();

                if new_mouse_interaction != window.mouse_interaction {
                    custom_actions.push(SessionShellAction::Mouse(new_mouse_interaction));
                    window.mouse_interaction = new_mouse_interaction;
                }

                let physical_size = window.state.physical_size();
                compositor.configure_surface(
                    &mut window.surface,
                    physical_size.width,
                    physical_size.height,
                );
                runtime.broadcast(iced_futures::subscription::Event::Interaction {
                    window: id,
                    event: redraw_event.clone(),
                    status: iced_core::event::Status::Ignored,
                });
                debug.render_started();

                debug.draw_started();
                ui.draw(
                    &mut window.renderer,
                    &application.theme(),
                    &iced_core::renderer::Style {
                        text_color: window.state.text_color(),
                    },
                    window.state.cursor(),
                );
                debug.draw_finished();
                compositor
                    .present(
                        &mut window.renderer,
                        &mut window.surface,
                        window.state.viewport(),
                        window.state.background_color(),
                        &debug.overlay(),
                    )
                    .ok();

                debug.render_finished();
            }
            MultiWindowIcedSessionLockEvent(Some(id), IcedSessionLockEvent::Window(event)) => {
                let Some((id, window)) = window_manager.get_mut_alias(id) else {
                    continue;
                };
                window.state.update(&event);
                if let Some(event) = conversion::window_event(&event, window.state.modifiers()) {
                    events.push((Some(id), event));
                }
            }
            MultiWindowIcedSessionLockEvent(_, IcedSessionLockEvent::UserEvent(action)) => {
                let mut cached_interfaces: HashMap<window::Id, user_interface::Cache> =
                    ManuallyDrop::into_inner(user_interfaces)
                        .drain()
                        .map(|(id, ui)| (id, ui.into_cache()))
                        .collect();

                run_action(
                    &application,
                    &mut messages,
                    &mut compositor,
                    action,
                    &mut clipboard,
                    &mut should_exit,
                    &mut debug,
                    &mut window_manager,
                    &mut cached_interfaces,
                );

                user_interfaces = ManuallyDrop::new(build_user_interfaces(
                    &application,
                    &mut debug,
                    &mut window_manager,
                    cached_interfaces,
                ));
                if should_exit {
                    break;
                }
            }
            MultiWindowIcedSessionLockEvent(_, IcedSessionLockEvent::NormalUpdate) => {
                if events.is_empty() && messages.is_empty() {
                    continue;
                }

                debug.event_processing_started();

                let mut uis_stale = false;
                for (id, window) in window_manager.iter_mut() {
                    let mut window_events = vec![];

                    events.retain(|(window_id, event)| {
                        if *window_id == Some(id) || window_id.is_none() {
                            window_events.push(event.clone());
                            false
                        } else {
                            true
                        }
                    });

                    if window_events.is_empty() && messages.is_empty() {
                        continue;
                    }
                    let (ui_state, statuses) = user_interfaces
                        .get_mut(&id)
                        .expect("Get user interface")
                        .update(
                            &window_events,
                            window.state.cursor(),
                            &mut window.renderer,
                            &mut clipboard,
                            &mut messages,
                        );

                    if !uis_stale {
                        uis_stale = matches!(ui_state, user_interface::State::Outdated);
                    }

                    debug.event_processing_finished();

                    for (event, status) in window_events.drain(..).zip(statuses.into_iter()) {
                        runtime.broadcast(iced_futures::subscription::Event::Interaction {
                            window: id,
                            event,
                            status,
                        });
                    }
                }
                // TODO mw application update returns which window IDs to update
                if !messages.is_empty() || uis_stale {
                    let cached_interfaces: HashMap<window::Id, user_interface::Cache> =
                        ManuallyDrop::into_inner(user_interfaces)
                            .drain()
                            .map(|(id, ui)| (id, ui.into_cache()))
                            .collect();

                    // Update application
                    update(&mut application, &mut runtime, &mut debug, &mut messages);

                    for (_id, window) in window_manager.iter_mut() {
                        window.state.synchronize(&application);
                    }

                    custom_actions.push(SessionShellAction::RedrawAll);

                    user_interfaces = ManuallyDrop::new(build_user_interfaces(
                        &application,
                        &mut debug,
                        &mut window_manager,
                        cached_interfaces,
                    ));
                }
            }
            _ => {}
        }

        let mut copyactions = vec![];
        std::mem::swap(&mut copyactions, &mut custom_actions);
        control_sender.start_send(copyactions).ok();
    }
    let _ = ManuallyDrop::into_inner(user_interfaces);
}

#[allow(clippy::type_complexity)]
pub fn build_user_interfaces<'a, A: Application, C>(
    application: &'a A,
    debug: &mut Debug,
    window_manager: &mut WindowManager<A, C>,
    mut cached_user_interfaces: HashMap<iced::window::Id, user_interface::Cache>,
) -> HashMap<iced::window::Id, UserInterface<'a, A::Message, A::Theme, A::Renderer>>
where
    C: Compositor<Renderer = A::Renderer>,
    A::Theme: DefaultStyle,
{
    cached_user_interfaces
        .drain()
        .filter_map(|(id, cache)| {
            let window = window_manager.get_mut(id)?;

            Some((
                id,
                build_user_interface(
                    application,
                    cache,
                    &mut window.renderer,
                    window.state.logical_size(),
                    debug,
                    id,
                ),
            ))
        })
        .collect()
}

/// Builds a [`UserInterface`] for the provided [`Application`], logging
/// [`struct@Debug`] information accordingly.
fn build_user_interface<'a, A: Application>(
    application: &'a A,
    cache: user_interface::Cache,
    renderer: &mut A::Renderer,
    size: Size,
    debug: &mut Debug,
    id: iced::window::Id,
) -> UserInterface<'a, A::Message, A::Theme, A::Renderer>
where
    A::Theme: DefaultStyle,
{
    debug.view_started();
    let view = application.view(id);
    debug.view_finished();

    debug.layout_started();
    let user_interface = UserInterface::build(view, size, cache, renderer);
    debug.layout_finished();
    user_interface
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn update<A: Application, E: Executor>(
    application: &mut A,
    runtime: &mut SessionRuntime<E, A::Message>,
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

    let subscription = runtime.enter(|| application.subscription());
    runtime.track(iced_futures::subscription::into_recipes(
        subscription.map(Action::Output),
    ));
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn run_action<A, C>(
    application: &A,
    messages: &mut Vec<A::Message>,
    compositor: &mut C,
    action: Action<A::Message>,
    clipboard: &mut SessionLockClipboard,
    should_exit: &mut bool,
    debug: &mut Debug,
    window_manager: &mut WindowManager<A, C>,
    ui_caches: &mut HashMap<iced::window::Id, user_interface::Cache>,
) where
    A: Application,
    C: Compositor<Renderer = A::Renderer> + 'static,
    A::Theme: DefaultStyle,
    A::Message: 'static + TryInto<UnLockAction, Error = A::Message>,
{
    use iced_core::widget::operation;
    use iced_runtime::clipboard;
    use iced_runtime::window;
    use iced_runtime::window::Action as WinowAction;
    //let mut customactions = Vec::new();
    match action {
        Action::Output(message) => match message.try_into() {
            Ok(action) => {
                let _: UnLockAction = action;
                *should_exit = true;
            }
            Err(message) => messages.push(message),
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
            let mut current_operation = Some(action);

            let mut uis = build_user_interfaces(
                application,
                debug,
                window_manager,
                std::mem::take(ui_caches),
            );

            'operate: while let Some(mut operation) = current_operation.take() {
                for (id, ui) in uis.iter_mut() {
                    if let Some(window) = window_manager.get_mut(*id) {
                        ui.operate(&window.renderer, operation.as_mut());

                        match operation.finish() {
                            operation::Outcome::None => {}
                            operation::Outcome::Some(_message) => {
                                //proxy.send_event(message).ok();

                                // operation completed, don't need to try to operate on rest of UIs
                                break 'operate;
                            }
                            operation::Outcome::Chain(next) => {
                                current_operation = Some(next);
                            }
                        }
                    }
                }
            }

            *ui_caches = uis.drain().map(|(id, ui)| (id, ui.into_cache())).collect();
        }
        Action::Window(action) => 'out: {
            match action {
                WinowAction::Close(_) => {
                    *should_exit = true;
                }
                WinowAction::Screenshot(id, channel) => {
                    let Some(window) = window_manager.get_mut(id) else {
                        break 'out;
                    };
                    let bytes = compositor.screenshot(
                        &mut window.renderer,
                        &mut window.surface,
                        window.state.viewport(),
                        window.state.background_color(),
                        &debug.overlay(),
                    );
                    let _ = channel.send(window::Screenshot::new(
                        bytes,
                        window.state.physical_size(),
                        window.state.viewport().scale_factor(),
                    ));
                }
                _ => {}
            }
        }
        Action::LoadFont { bytes, channel } => {
            // TODO: Error handling (?)
            compositor.load_font(bytes.clone());

            let _ = channel.send(Ok(()));
        }

        _ => {}
    }
}
