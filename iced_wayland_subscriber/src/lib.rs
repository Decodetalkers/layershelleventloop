use futures::SinkExt;
use wayland_client::{
    delegate_noop,
    globals::{registry_queue_init, GlobalListContents},
    protocol::{
        wl_output::{self, WlOutput},
        wl_registry,
    },
    Connection, Dispatch, Proxy,
};

use wayland_protocols::xdg::xdg_output::zv1::client::{
    zxdg_output_manager_v1::ZxdgOutputManagerV1, zxdg_output_v1,
};

#[derive(Debug)]
struct BaseState;

// so interesting, it is just need to invoke once, it just used to get the globals
impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for BaseState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_registry::WlRegistry,
        _event: <wl_registry::WlRegistry as wayland_client::Proxy>::Event,
        _data: &GlobalListContents,
        _conn: &Connection,
        _qh: &wayland_client::QueueHandle<Self>,
    ) {
    }
}

#[derive(Debug, Default)]
struct SubscribeState {
    events: Vec<WaylandEvents>,
    padding_wloutputs: Vec<wl_output::WlOutput>,
}

impl Dispatch<wl_registry::WlRegistry, ()> for SubscribeState {
    fn event(
        state: &mut Self,
        proxy: &wl_registry::WlRegistry,
        event: <wl_registry::WlRegistry as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        qh: &wayland_client::QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } => {
                if interface == wl_output::WlOutput::interface().name {
                    let output = proxy.bind::<wl_output::WlOutput, _, _>(name, version, qh, ());
                    state.padding_wloutputs.push(output);
                }
            }
            wl_registry::Event::GlobalRemove { .. } => {}
            _ => unreachable!(),
        }
    }
}
impl Dispatch<zxdg_output_v1::ZxdgOutputV1, ()> for SubscribeState {
    fn event(
        state: &mut Self,
        _proxy: &zxdg_output_v1::ZxdgOutputV1,
        event: <zxdg_output_v1::ZxdgOutputV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &wayland_client::QueueHandle<Self>,
    ) {
        if let zxdg_output_v1::Event::Name { name } = event {
            state.events.push(WaylandEvents::OutputInsert(name));
        }
    }
}
delegate_noop!(SubscribeState: ignore WlOutput); // output is need to place layer_shell, although here
delegate_noop!(SubscribeState: ignore ZxdgOutputManagerV1);

#[derive(Debug, Clone)]
pub enum WaylandEvents {
    OutputInsert(String),
}

pub fn listen() -> iced::Subscription<WaylandEvents> {
    iced::Subscription::run(|| {
        iced::stream::channel(100, |mut output| async move {
            let connection = Connection::connect_to_env().unwrap();
            let (globals, _) = registry_queue_init::<BaseState>(&connection).unwrap(); // We just need the
                                                                                       // global, the
                                                                                       // event_queue is
                                                                                       // not needed, we
                                                                                       // do not need
                                                                                       // BaseState after

            let mut state = SubscribeState::default();

            let mut event_queue = connection.new_event_queue::<SubscribeState>();
            let qhandle = event_queue.handle();
            let display = connection.display();

            let xdg_output_manager = globals
                .bind::<ZxdgOutputManagerV1, _, _>(&qhandle, 1..=3, ())
                .unwrap(); // b
            display.get_registry(&qhandle, ());
            loop {
                event_queue.blocking_dispatch(&mut state).unwrap();
                let mut current_outputs = vec![];
                std::mem::swap(&mut current_outputs, &mut state.padding_wloutputs);
                for output in current_outputs {
                    xdg_output_manager.get_xdg_output(&output, &qhandle, ());
                }

                let mut current_events = vec![];
                std::mem::swap(&mut current_events, &mut state.events);
                for event in current_events {
                    output.send(event).await.ok();
                }
                async_io::Timer::after(std::time::Duration::from_millis(10)).await;
            }
        })
    })
}
