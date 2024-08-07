mod state;
use crate::{
    actions::{
        IcedNewMenuSettings, IcedNewPopupSettings, LayershellCustomActionsWithIdAndInfo,
        LayershellCustomActionsWithIdInner, MenuDirection,
    },
    multi_window::window_manager::WindowManager,
    settings::VirtualKeyboardSettings,
};
use std::{collections::HashMap, f64, mem::ManuallyDrop, os::fd::AsFd, sync::Arc, time::Duration};

use crate::{
    actions::{LayerShellActions, LayershellCustomActionsWithInfo},
    clipboard::LayerShellClipboard,
    conversion,
    error::Error,
};

use iced_graphics::Compositor;

use iced_core::{time::Instant, Size};

use iced_runtime::{multi_window::Program, user_interface, Command, Debug, UserInterface};

use iced_style::application::StyleSheet;

use iced_futures::{Executor, Runtime, Subscription};

use layershellev::{
    calloop::timer::{TimeoutAction, Timer},
    reexport::zwp_virtual_keyboard_v1,
    LayerEvent, NewPopUpSettings, ReturnData, WindowState,
};

use futures::{channel::mpsc, SinkExt, StreamExt};

use crate::{
    event::{IcedLayerEvent, MultiWindowIcedLayerEvent},
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
/// [`Command`] in some of its methods.
///
/// When using an [`Application`] with the `debug` feature enabled, a debug view
/// can be toggled by pressing `F12`.
pub trait Application: Program
where
    Self::Theme: StyleSheet,
{
    /// The data needed to initialize your [`Application`].
    type Flags;

    type WindowInfo;

    /// Initializes the [`Application`] with the flags provided to
    /// [`run`] as part of the [`Settings`].
    ///
    /// Here is where you should return the initial state of your app.
    ///
    /// Additionally, you can return a [`Command`] if you need to perform some
    /// async action in the background on startup. This is useful if you want to
    /// load state from a file, perform an initial HTTP request, etc.
    fn new(flags: Self::Flags) -> (Self, Command<Self::Message>);

    fn namespace(&self) -> String;
    /// Returns the current title of the [`Application`].
    ///
    /// This title can be dynamic! The runtime will automatically update the
    /// title of your application when necessary.
    fn title(&self) -> String {
        self.namespace()
    }

    fn id_info(&self, _id: iced_core::window::Id) -> Option<&Self::WindowInfo>;

    fn set_id_info(&mut self, _id: iced_core::window::Id, info: Self::WindowInfo);
    fn remove_id(&mut self, _id: iced_core::window::Id);

    /// Returns the current [`Theme`] of the [`Application`].
    fn theme(&self) -> Self::Theme;

    /// Returns the [`Style`] variation of the [`Theme`].
    fn style(&self) -> <Self::Theme as StyleSheet>::Style {
        Default::default()
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

// a dispatch loop, another is listen loop
pub fn run<A, E, C>(
    settings: Settings<A::Flags>,
    compositor_settings: C::Settings,
) -> Result<(), Error>
where
    A: Application + 'static,
    E: Executor + 'static,
    C: Compositor<Renderer = A::Renderer> + 'static,
    A::Theme: StyleSheet,
    <A as Application>::WindowInfo: Clone,
{
    use futures::task;
    use futures::Future;
    use iced::window;

    let mut debug = Debug::new();
    debug.startup_started();

    let (message_sender, message_receiver) = std::sync::mpsc::channel::<A::Message>();

    let proxy = IcedProxy::new(message_sender);
    let runtime: Runtime<E, IcedProxy<A::Message>, <A as Program>::Message> = {
        let executor = E::new().map_err(Error::ExecutorCreationFailed)?;

        Runtime::new(executor, proxy.clone())
    };

    let (application, init_command) = {
        let flags = settings.flags;

        runtime.enter(|| A::new(flags))
    };

    let ev: WindowState<A::WindowInfo> = layershellev::WindowState::new(&application.namespace())
        .with_single(false)
        .with_use_display_handle(true)
        .with_option_size(settings.layer_settings.size)
        .with_layer(settings.layer_settings.layer)
        .with_anchor(settings.layer_settings.anchor)
        .with_exclusize_zone(settings.layer_settings.exclusize_zone)
        .with_margin(settings.layer_settings.margins)
        .with_keyboard_interacivity(settings.layer_settings.keyboard_interactivity)
        .build()
        .unwrap();

    let window = Arc::new(ev.gen_main_wrapper());
    let mut compositor = C::new(compositor_settings, window.clone())?;

    let mut window_manager = WindowManager::new();
    let _ = window_manager.insert(
        window::Id::MAIN,
        ev.main_window().get_size(),
        window,
        &application,
        &mut compositor,
    );

    let (mut event_sender, event_receiver) =
        mpsc::unbounded::<MultiWindowIcedLayerEvent<A::Message, A::WindowInfo>>();
    let (control_sender, mut control_receiver) =
        mpsc::unbounded::<Vec<LayerShellActions<A::WindowInfo>>>();

    let mut instance = Box::pin(run_instance::<A, E, C>(
        application,
        compositor,
        runtime,
        proxy,
        debug,
        event_receiver,
        control_sender,
        //state,
        window_manager,
        init_command,
    ));

    let mut context = task::Context::from_waker(task::noop_waker_ref());

    let mut pointer_serial: u32 = 0;

    let _ = ev.running_with_proxy(message_receiver, move |event, ev, index| {
        use layershellev::DispatchMessage;
        let mut def_returndata = ReturnData::None;
        let sended_id = index.map(|index| ev.get_unit(index).id());
        match event {
            LayerEvent::InitRequest => {
                if settings.virtual_keyboard_support.is_some() {
                    def_returndata = ReturnData::RequestBind;
                }
            }
            LayerEvent::BindProvide(globals, qh) => {
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
            LayerEvent::RequestMessages(message) => 'outside: {
                match message {
                    DispatchMessage::RequestRefresh {
                        width,
                        height,
                        is_created,
                    } => {
                        let unit = ev.get_unit(index.unwrap());
                        event_sender
                            .start_send(MultiWindowIcedLayerEvent(
                                sended_id,
                                IcedLayerEvent::RequestRefreshWithWrapper {
                                    width: *width,
                                    height: *height,
                                    wrapper: unit.gen_wrapper(),
                                    is_created: *is_created,
                                    info: unit.get_binding().cloned(),
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
                    .start_send(MultiWindowIcedLayerEvent(sended_id, message.into()))
                    .expect("Cannot send");
            }

            LayerEvent::UserEvent(event) => {
                event_sender
                    .start_send(MultiWindowIcedLayerEvent(
                        sended_id,
                        IcedLayerEvent::UserEvent(event),
                    ))
                    .ok();
            }
            LayerEvent::NormalDispatch => {
                event_sender
                    .start_send(MultiWindowIcedLayerEvent(sended_id, IcedLayerEvent::NormalUpdate))
                    .expect("Cannot send");
            }
            _ => {}
        }
        let poll = instance.as_mut().poll(&mut context);
        match poll {
            task::Poll::Pending => {
                if let Ok(Some(flow)) = control_receiver.try_next() {
                    for flow in flow {
                        match flow {
                            LayerShellActions::CustomActionsWithId(actions) => {
                                for LayershellCustomActionsWithIdInner(id, option_id, action) in
                                    actions
                                {
                                    let Some(window) = ev.get_window_with_id(id) else {
                                        continue;
                                    };
                                    match action {
                                        LayershellCustomActionsWithInfo::AnchorChange(anchor) => {
                                            window.set_anchor(anchor);
                                        }
                                        LayershellCustomActionsWithInfo::LayerChange(layer) => {
                                            window.set_layer(layer);
                                        }
                                        LayershellCustomActionsWithInfo::SizeChange((width, height)) => {
                                            window.set_size((width, height));
                                        }
                                        LayershellCustomActionsWithInfo::VirtualKeyboardPressed {
                                            time,
                                            key,
                                        } => {
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
                                        LayershellCustomActionsWithInfo::NewLayerShell((
                                            settings,
                                            info,
                                        )) => {
                                            return ReturnData::NewLayerShell((
                                                settings,
                                                Some(info),
                                            ));
                                        }
                                        LayershellCustomActionsWithInfo::RemoveLayerShell(id) => {
                                            event_sender.start_send(MultiWindowIcedLayerEvent(None, IcedLayerEvent::WindowRemoved(id))).ok();
                                            return ReturnData::RemoveLayershell(option_id.unwrap())
                                        }
                                        LayershellCustomActionsWithInfo::NewPopUp((menusettings, info)) => {
                                            let IcedNewPopupSettings { size, position } = menusettings;
                                            let Some(id) = ev.current_surface_id() else {
                                                continue;
                                            };
                                            let popup_settings = NewPopUpSettings {size, position,id};
                                            return ReturnData::NewPopUp((
                                                popup_settings,
                                                Some(info),
                                            ))
                                        }
                                        LayershellCustomActionsWithInfo::NewMenu((menusetting, info)) => {
                                            let Some(id) = ev.current_surface_id() else {
                                                continue;
                                            };
                                            event_sender
                                                .start_send(MultiWindowIcedLayerEvent(Some(id), IcedLayerEvent::NewMenu((menusetting, info))))
                                                .expect("Cannot send");
                                        }
                                    }
                                }
                            }
                            LayerShellActions::NewMenu((menusettings, info)) => {
                                let IcedNewPopupSettings { size, position } = menusettings;
                                let Some(id) = ev.current_surface_id() else {
                                    continue;
                                };
                                let popup_settings = NewPopUpSettings {
                                    size,
                                    position,
                                    id
                                };
                                return ReturnData::NewPopUp((
                                    popup_settings,
                                    Some(info),
                                ))
                            }
                            LayerShellActions::Mouse(mouse) => {
                                let Some(pointer) = ev.get_pointer() else {
                                    return ReturnData::None;
                                };

                                return ReturnData::RequestSetCursorShape((
                                    conversion::mouse_interaction(mouse),
                                    pointer.clone(),
                                    pointer_serial,
                                ));
                            }
                            LayerShellActions::RedrawAll => {
                                return ReturnData::RedrawAllRequest;
                            }
                            LayerShellActions::RedrawWindow(index) => {
                                return ReturnData::RedrawIndexRequest(index);
                            }
                            _ => {}
                        }
                    }
                }
                def_returndata
            }
            task::Poll::Ready(_) => ReturnData::RequestExist,
        }
    });
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn run_instance<A, E, C>(
    mut application: A,
    mut compositor: C,
    mut runtime: Runtime<E, IcedProxy<A::Message>, A::Message>,
    mut proxy: IcedProxy<A::Message>,
    mut debug: Debug,
    mut event_receiver: mpsc::UnboundedReceiver<
        MultiWindowIcedLayerEvent<A::Message, A::WindowInfo>,
    >,
    mut control_sender: mpsc::UnboundedSender<Vec<LayerShellActions<A::WindowInfo>>>,
    mut window_manager: WindowManager<A, C>,
    init_command: Command<A::Message>,
) where
    A: Application + 'static,
    E: Executor + 'static,
    C: Compositor<Renderer = A::Renderer> + 'static,
    A::Theme: StyleSheet,
    A::WindowInfo: Clone,
{
    use iced::window;
    use iced_core::Event;
    let main_window = window_manager
        .get_mut(window::Id::MAIN)
        .expect("Get main window");
    let main_window_size = main_window.state.logical_size();
    let mut clipboard = LayerShellClipboard;
    let mut ui_caches: HashMap<window::Id, user_interface::Cache> = HashMap::new();

    let mut user_interfaces = ManuallyDrop::new(build_user_interfaces(
        &application,
        &mut debug,
        &mut window_manager,
        HashMap::from_iter([(window::Id::MAIN, user_interface::Cache::default())]),
    ));

    let mut events = {
        vec![(
            Some(window::Id::MAIN),
            Event::Window(
                window::Id::MAIN,
                window::Event::Opened {
                    position: None,
                    size: main_window_size,
                },
            ),
        )]
    };
    let mut custom_actions = Vec::new();

    let mut should_exit = false;
    let mut messages = Vec::new();

    run_command(
        &application,
        &mut compositor,
        init_command,
        &mut runtime,
        &mut custom_actions,
        &mut should_exit,
        &mut proxy,
        &mut debug,
        &mut window_manager,
        &mut ui_caches,
    );

    runtime.track(application.subscription().into_recipes());
    while let Some(event) = event_receiver.next().await {
        match event {
            MultiWindowIcedLayerEvent(
                _id,
                IcedLayerEvent::RequestRefreshWithWrapper {
                    width,
                    height,
                    wrapper,
                    is_created,
                    info,
                },
            ) => {
                let mut is_new_window = false;
                let (id, window) = if window_manager.get_mut_alias(wrapper.id()).is_none() {
                    is_new_window = true;
                    let id = window::Id::unique();

                    let window = window_manager.insert(
                        id,
                        (width, height),
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
                        Event::Window(
                            id,
                            window::Event::Opened {
                                position: None,
                                size: window.state.logical_size(),
                            },
                        ),
                    ));
                    (id, window)
                } else {
                    let (id, window) = window_manager.get_mut_alias(wrapper.id()).unwrap();
                    let ui = user_interfaces.remove(&id).expect("Get User interface");
                    window.state.update_view_port(width, height);
                    #[allow(unused)]
                    let renderer = &window.renderer;
                    let _ = user_interfaces.insert(
                        id,
                        ui.relayout(
                            Size {
                                width: width as f32,
                                height: height as f32,
                            },
                            &mut window.renderer,
                        ),
                    );
                    (id, window)
                };

                let ui = user_interfaces.get_mut(&id).expect("Get User interface");

                let redraw_event =
                    Event::Window(id, window::Event::RedrawRequested(Instant::now()));

                let cursor = window.state.cursor();

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
                    custom_actions.push(LayerShellActions::Mouse(new_mouse_interaction));
                    window.mouse_interaction = new_mouse_interaction;
                }

                compositor.configure_surface(&mut window.surface, width, height);
                runtime.broadcast(redraw_event.clone(), iced_core::event::Status::Ignored);
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
                if !is_new_window {
                    compositor
                        .present(
                            &mut window.renderer,
                            &mut window.surface,
                            window.state.viewport(),
                            window.state.background_color(),
                            &debug.overlay(),
                        )
                        .ok();
                }

                debug.render_finished();

                if is_created {
                    let cached_interfaces: HashMap<window::Id, user_interface::Cache> =
                        ManuallyDrop::into_inner(user_interfaces)
                            .drain()
                            .map(|(id, ui)| (id, ui.into_cache()))
                            .collect();
                    application.set_id_info(id, info.unwrap().clone());
                    user_interfaces = ManuallyDrop::new(build_user_interfaces(
                        &application,
                        &mut debug,
                        &mut window_manager,
                        cached_interfaces,
                    ));
                }
            }
            MultiWindowIcedLayerEvent(Some(id), IcedLayerEvent::Window(event)) => {
                let Some((id, window)) = window_manager.get_mut_alias(id) else {
                    continue;
                };
                window.state.update(&event);
                if let Some(event) = conversion::window_event(id, &event, window.state.modifiers())
                {
                    events.push((Some(id), event));
                }
            }
            MultiWindowIcedLayerEvent(_, IcedLayerEvent::UserEvent(event)) => {
                messages.push(event);
            }
            MultiWindowIcedLayerEvent(_, IcedLayerEvent::NormalUpdate) => {
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
                        runtime.broadcast(event, status);
                    }
                }
                // TODO mw application update returns which window IDs to update
                if !messages.is_empty() || uis_stale {
                    let mut cached_interfaces: HashMap<window::Id, user_interface::Cache> =
                        ManuallyDrop::into_inner(user_interfaces)
                            .drain()
                            .map(|(id, ui)| (id, ui.into_cache()))
                            .collect();

                    // Update application
                    update(
                        &mut application,
                        &mut compositor,
                        &mut runtime,
                        &mut should_exit,
                        &mut proxy,
                        &mut debug,
                        &mut messages,
                        &mut custom_actions,
                        &mut window_manager,
                        &mut cached_interfaces,
                    );

                    for (_id, window) in window_manager.iter_mut() {
                        window.state.synchronize(&application);
                    }

                    custom_actions.push(LayerShellActions::RedrawAll);

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
            }
            MultiWindowIcedLayerEvent(_, IcedLayerEvent::WindowRemoved(id)) => {
                let cached_interfaces: HashMap<window::Id, user_interface::Cache> =
                    ManuallyDrop::into_inner(user_interfaces)
                        .drain()
                        .map(|(id, ui)| (id, ui.into_cache()))
                        .collect();
                application.remove_id(id);
                user_interfaces = ManuallyDrop::new(build_user_interfaces(
                    &application,
                    &mut debug,
                    &mut window_manager,
                    cached_interfaces,
                ));
            }
            MultiWindowIcedLayerEvent(
                Some(id),
                IcedLayerEvent::NewMenu((
                    IcedNewMenuSettings {
                        size: (width, height),
                        direction,
                    },
                    info,
                )),
            ) => {
                let Some((_, window)) = window_manager.get_alias(id) else {
                    continue;
                };

                let Some(point) = window.state.mouse_position() else {
                    continue;
                };

                let (x, mut y) = (point.x as i32, point.y as i32);
                if let MenuDirection::Up = direction {
                    y -= height as i32;
                }
                custom_actions.push(LayerShellActions::NewMenu((
                    IcedNewPopupSettings {
                        size: (width, height),
                        position: (x, y),
                    },
                    info,
                )));
            }
            _ => {}
        }
        control_sender.start_send(custom_actions.clone()).ok();
        custom_actions.clear();
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
    A::Theme: StyleSheet,
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
    A::Theme: StyleSheet,
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
pub(crate) fn update<A: Application, C, E: Executor>(
    application: &mut A,
    compositor: &mut C,
    runtime: &mut Runtime<E, IcedProxy<A::Message>, A::Message>,
    should_exit: &mut bool,
    proxy: &mut IcedProxy<A::Message>,
    debug: &mut Debug,
    messages: &mut Vec<A::Message>,
    custom_actions: &mut Vec<LayerShellActions<A::WindowInfo>>,
    window_manager: &mut WindowManager<A, C>,
    ui_caches: &mut HashMap<iced::window::Id, user_interface::Cache>,
) where
    C: Compositor<Renderer = A::Renderer> + 'static,
    A::Theme: StyleSheet,
    A::Message: 'static,
    A::WindowInfo: Clone + 'static,
{
    for message in messages.drain(..) {
        debug.log_message(&message);

        debug.update_started();
        let command: Command<A::Message> = runtime.enter(|| application.update(message));
        debug.update_finished();

        run_command(
            application,
            compositor,
            command,
            runtime,
            custom_actions,
            should_exit,
            proxy,
            debug,
            window_manager,
            ui_caches,
        );
    }

    let subscription = application.subscription();
    runtime.track(subscription.into_recipes());
}

#[allow(unused)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_command<A, C, E>(
    application: &A,
    compositor: &mut C,
    command: Command<A::Message>,
    runtime: &mut Runtime<E, IcedProxy<A::Message>, A::Message>,
    custom_actions: &mut Vec<LayerShellActions<A::WindowInfo>>,
    should_exit: &mut bool,
    proxy: &mut IcedProxy<A::Message>,
    debug: &mut Debug,
    window_manager: &mut WindowManager<A, C>,
    ui_caches: &mut HashMap<iced::window::Id, user_interface::Cache>,
) where
    A: Application,
    E: Executor,
    C: Compositor<Renderer = A::Renderer> + 'static,
    A::Theme: StyleSheet,
    A::Message: 'static,
    A::WindowInfo: Clone + 'static,
{
    use iced_core::widget::operation;
    use iced_runtime::command;
    use iced_runtime::window;
    use iced_runtime::window::Action as WinowAction;
    let mut customactions = Vec::new();
    for action in command.actions() {
        match action {
            command::Action::Future(future) => {
                runtime.spawn(future);
            }
            command::Action::Stream(stream) => {
                runtime.run(stream);
            }
            command::Action::Clipboard(_action) => {
                // TODO:
            }
            command::Action::Widget(action) => {
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
                                operation::Outcome::Some(message) => {
                                    proxy.send(message);

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
            command::Action::Window(action) => match action {
                WinowAction::Close(id) => {
                    if id == iced::window::Id::MAIN {
                        *should_exit = true;
                        continue;
                    }
                    if let Some(layerid) = window_manager.get_layer_id(id) {
                        customactions.push(LayershellCustomActionsWithIdInner(
                            layerid,
                            Some(layerid),
                            LayershellCustomActionsWithInfo::RemoveLayerShell(id),
                        ))
                    }
                }
                WinowAction::Screenshot(id, tag) => {
                    let Some(window) = window_manager.get_mut(id) else {
                        continue;
                    };
                    let bytes = compositor.screenshot(
                        &mut window.renderer,
                        &mut window.surface,
                        window.state.viewport(),
                        window.state.background_color(),
                        &debug.overlay(),
                    );

                    proxy.send(tag(window::Screenshot::new(
                        bytes,
                        window.state.physical_size(),
                    )));
                }
                _ => {}
            },
            command::Action::LoadFont { bytes, tagger } => {
                use iced_core::text::Renderer;

                // TODO change this once we change each renderer to having a single backend reference.. :pain:
                // TODO: Error handling (?)
                for (_, window) in window_manager.iter_mut() {
                    window.renderer.load_font(bytes.clone());
                }

                proxy.send(tagger(Ok(())));
            }
            command::Action::Custom(custom) => {
                if let Some(action) =
                    custom.downcast_ref::<LayershellCustomActionsWithIdAndInfo<A::WindowInfo>>()
                {
                    let option_id =
                        if let LayershellCustomActionsWithInfo::RemoveLayerShell(id) = action.1 {
                            window_manager.get_layer_id(id)
                        } else {
                            None
                        };
                    if let Some(id) = window_manager.get_layer_id(action.0) {
                        customactions.push(LayershellCustomActionsWithIdInner(
                            id,
                            option_id,
                            action.1.clone(),
                        ));
                    }
                } else if let Some(action) =
                    custom.downcast_ref::<LayershellCustomActionsWithIdAndInfo<()>>()
                {
                    // NOTE: try to unwrap again, if with type LayershellCustomActionsWithInfo<()>,
                    let option_id =
                        if let LayershellCustomActionsWithInfo::RemoveLayerShell(id) = action.1 {
                            window_manager.get_layer_id(id)
                        } else {
                            None
                        };
                    let turnaction: LayershellCustomActionsWithInfo<A::WindowInfo> = match action.1
                    {
                        LayershellCustomActionsWithInfo::AnchorChange(anchor) => {
                            LayershellCustomActionsWithInfo::AnchorChange(anchor)
                        }
                        LayershellCustomActionsWithInfo::LayerChange(layer) => {
                            LayershellCustomActionsWithInfo::LayerChange(layer)
                        }
                        LayershellCustomActionsWithInfo::SizeChange(size) => {
                            LayershellCustomActionsWithInfo::SizeChange(size)
                        }
                        LayershellCustomActionsWithInfo::VirtualKeyboardPressed { time, key } => {
                            LayershellCustomActionsWithInfo::VirtualKeyboardPressed { time, key }
                        }
                        LayershellCustomActionsWithInfo::RemoveLayerShell(id) => {
                            LayershellCustomActionsWithInfo::RemoveLayerShell(id)
                        }
                        _ => {
                            continue;
                        }
                    };
                    if let Some(id) = window_manager.get_layer_id(action.0) {
                        customactions.push(LayershellCustomActionsWithIdInner(
                            id, option_id, turnaction,
                        ));
                    }
                }
            }
            _ => {}
        }
    }
    custom_actions.push(LayerShellActions::CustomActionsWithId(customactions));
}
