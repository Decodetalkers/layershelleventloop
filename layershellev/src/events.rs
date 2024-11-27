use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::Layer,
    zwlr_layer_surface_v1::{Anchor, KeyboardInteractivity},
};

use wayland_client::{
    globals::GlobalList,
    protocol::{
        wl_buffer::WlBuffer,
        wl_compositor::WlCompositor,
        wl_output::WlOutput,
        wl_pointer::{self, ButtonState, WlPointer},
        wl_shm::WlShm,
    },
    QueueHandle, WEnum,
};

use crate::{id, xkb_keyboard::KeyEvent};

use crate::keyboard::ModifiersState;

use super::WindowState;

use crate::id::Id;

use std::{fmt::Debug, fs::File};

/// tell program what event is happened
///
/// InitRequest will tell the program is inited, you can request to Bind other wayland-protocols
/// there, with return [ReturnData::RequestBind]
///
/// RequestBuffer request to get the wl-buffer, so you init a buffer_pool here. It return a
/// GlobalList and a QueueHandle. This will enough for bind a extra wayland-protocol, and also,
/// seat can be gotten directly from [WindowState]
///
/// RequestMessages store the DispatchMessage, you can know what happened during dispatch with this
/// event.
pub enum LayerEvent<'a, T, Message> {
    /// the first event when start a new gui, program. you can return [ReturnData::None] or
    /// [ReturnData::RequestBind], then it will continue to the next request.
    /// Here only the above two [ReturnData] are acceptable.
    InitRequest,
    /// if the info of the XdgOutput is changed, it will send the event
    XdgInfoChanged(XdgInfoChangedType),
    /// After you return [ReturnData::RequestBind] in the [LayerEvent::InitRequest] stage, next
    /// event is [LayerEvent::BindProvide], you can use the GlobalList and QueueHandle to create
    /// new wayland objects.
    BindProvide(&'a GlobalList, &'a QueueHandle<WindowState<T>>),
    /// After you return [ReturnData::RequestCompositor] in the init stage, next
    /// event is [LayerEvent::CompositorProvide], you can use the WlCompositor and QueueHandle to
    /// create new wayland objects.
    CompositorProvide(&'a WlCompositor, &'a QueueHandle<WindowState<T>>),
    /// create a new buffer after request. if you use display_handle, you do not need to care about
    /// it.
    RequestBuffer(
        &'a mut File,
        &'a WlShm,
        &'a QueueHandle<WindowState<T>>,
        u32,
        u32,
    ),
    /// Some thing KeyboardEvent, TouchEvent, MouseEvent and etc.
    RequestMessages(&'a DispatchMessage),
    /// Nothing happened, you can do some other things after it, like to refresh the ui, and etc.
    NormalDispatch,
    /// It return the event you passed with message_receiver, and return it back.
    UserEvent(Message),
}

/// This allow the new layershell can be selected on target output
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayerOutputSetting {
    ChosenOutput(String),
    FollowLastOutput,
    None,
}

/// layershell settings to create a new layershell surface
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewLayerShellSettings {
    /// the size of the layershell, optional.
    pub size: Option<(u32, u32)>,
    pub layer: Layer,
    pub anchor: Anchor,
    pub exclusive_zone: Option<i32>,
    pub margin: Option<(i32, i32, i32, i32)>,
    pub keyboard_interactivity: KeyboardInteractivity,
    /// follow the last output of the activated surface, used to create some thing like mako, who
    /// will show on the same window, only when the notifications is cleared, it will change the
    /// wl_output.
    pub events_transparent: bool,
    pub output_setting: LayerOutputSetting,
}

/// be used to create a new popup
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct NewPopUpSettings {
    /// the size of the popup
    pub size: (u32, u32),
    /// the position of the popup, relative to the he layersurface
    pub position: (i32, i32),
    /// It means where the popup is, on which surface. It is the id of that layershell
    pub id: id::Id,
}

impl Default for NewLayerShellSettings {
    fn default() -> Self {
        NewLayerShellSettings {
            anchor: Anchor::Bottom | Anchor::Left | Anchor::Right,
            layer: Layer::Top,
            exclusive_zone: None,
            size: None,
            margin: Some((0, 0, 0, 0)),
            keyboard_interactivity: KeyboardInteractivity::OnDemand,
            events_transparent: false,
            output_setting: LayerOutputSetting::None,
        }
    }
}

/// the return data
/// Note: when event is RequestBuffer, you must return WlBuffer
/// Note: when receive InitRequest, you can request to bind extra wayland-protocols. this time you
/// can bind virtual-keyboard. you can take startcolorkeyboard as reference, or the simple.rs. Also,
/// it should can bind with text-input, but I am not fully understand about this, maybe someone
/// familiar with it can do
///
/// When send RequestExit, it will tell the event to finish.
///
/// When send RequestSetCursorShape, you can set current pointer shape. please take
/// [cursor-shape](https://wayland.app/protocols/cursor-shape-v1#wp_cursor_shape_device_v1:enum:shape) as reference.
///
/// None means nothing will happened, no request, and no return data
#[derive(Debug, PartialEq, Eq)]
pub enum ReturnData<INFO> {
    WlBuffer(WlBuffer),
    RequestBind,
    RequestExit,
    RequestCompositor,
    RedrawAllRequest,
    RedrawIndexRequest(Id),
    RequestSetCursorShape((String, WlPointer, u32)),
    NewLayerShell((NewLayerShellSettings, Option<INFO>)),
    NewPopUp((NewPopUpSettings, Option<INFO>)),
    None,
}

/// this tell the what kind of information passed by [LayerEvent::XdgInfoChanged]
#[derive(Debug, Clone, Copy)]
pub enum XdgInfoChangedType {
    Position,
    Size,
    Name,
    Description,
}

/// Describes a scroll along one axis
#[derive(Default, Debug, Clone, Copy, PartialEq)]
pub struct AxisScroll {
    /// The scroll measured in pixels.
    pub absolute: f64,

    /// The scroll measured in steps.
    ///
    /// Note: this might always be zero if the scrolling is due to a touchpad or other continuous
    /// source.
    pub discrete: i32,

    /// The scroll was stopped.
    ///
    /// Generally this is encountered when hardware indicates the end of some continuous scrolling.
    pub stop: bool,
}

#[allow(unused)]
#[derive(Debug, Clone)]
pub(crate) enum DispatchMessageInner {
    NewDisplay(WlOutput),
    MouseButton {
        state: WEnum<ButtonState>,
        serial: u32,
        button: u32,
        time: u32,
    },
    MouseLeave,
    MouseEnter {
        pointer: WlPointer,
        serial: u32,
        surface_x: f64,
        surface_y: f64,
    },
    MouseMotion {
        time: u32,
        surface_x: f64,
        surface_y: f64,
    },
    Axis {
        time: u32,
        scale: f64,
        horizontal: AxisScroll,
        vertical: AxisScroll,
        source: Option<wl_pointer::AxisSource>,
    },
    TouchDown {
        serial: u32,
        time: u32,
        id: i32,
        x: f64,
        y: f64,
    },
    TouchUp {
        serial: u32,
        time: u32,
        id: i32,
        x: f64,
        y: f64,
    },
    TouchMotion {
        time: u32,
        id: i32,
        x: f64,
        y: f64,
    },
    TouchCancel {
        id: i32,
        x: f64,
        y: f64,
    },

    ModifiersChanged(ModifiersState),
    KeyboardInput {
        event: KeyEvent,

        /// If `true`, the event was generated synthetically by winit
        /// in one of the following circumstances:
        ///
        /// * Synthetic key press events are generated for all keys pressed when a window gains
        ///   focus. Likewise, synthetic key release events are generated for all keys pressed when
        ///   a window goes out of focus. ***Currently, this is only functional on X11 and
        ///   Windows***
        ///
        /// Otherwise, this value is always `false`.
        is_synthetic: bool,
    },
    RefreshSurface {
        width: u32,
        height: u32,
    },
    RequestRefresh {
        width: u32,
        height: u32,
        scale_float: f64,
        is_created: bool,
    },
    PreferredScale {
        scale_u32: u32,
        scale_float: f64,
    },
    XdgInfoChanged(XdgInfoChangedType),
}

/// This tell the DispatchMessage by dispatch
#[derive(Debug)]
pub enum DispatchMessage {
    /// forward the event of wayland-mouse
    MouseButton {
        state: WEnum<ButtonState>,
        serial: u32,
        button: u32,
        time: u32,
    },
    /// Mouse leave the surface
    MouseLeave,
    /// forward the event of wayland-mouse
    MouseEnter {
        pointer: WlPointer,
        serial: u32,
        surface_x: f64,
        surface_y: f64,
    },
    /// forward the event of wayland-mouse
    MouseMotion {
        time: u32,
        surface_x: f64,
        surface_y: f64,
    },
    /// About the scroll
    Axis {
        time: u32,
        scale: f64,
        horizontal: AxisScroll,
        vertical: AxisScroll,
        source: Option<wl_pointer::AxisSource>,
    },
    /// forward the event of wayland-touch
    TouchDown {
        serial: u32,
        time: u32,
        id: i32,
        x: f64,
        y: f64,
    },
    /// forward the event of wayland-touch
    TouchUp {
        serial: u32,
        time: u32,
        id: i32,
        x: f64,
        y: f64,
    },
    /// forward the event of wayland-touch
    TouchMotion { time: u32, id: i32, x: f64, y: f64 },
    /// TouchEvent is cancelled
    TouchCancel { id: i32, x: f64, y: f64 },
    /// Keyboard ModifiersChanged.
    ModifiersChanged(ModifiersState),
    /// Keyboard Event about input.
    KeyboardInput {
        event: KeyEvent,

        /// If `true`, the event was generated synthetically by winit
        /// in one of the following circumstances:
        ///
        /// * Synthetic key press events are generated for all keys pressed when a window gains
        ///   focus. Likewise, synthetic key release events are generated for all keys pressed when
        ///   a window goes out of focus. ***Currently, this is only functional on X11 and
        ///   Windows***
        ///
        /// Otherwise, this value is always `false`.
        is_synthetic: bool,
    },
    /// this will request to do refresh the whole screen, because the layershell tell that a new
    /// configure happened
    RequestRefresh {
        width: u32,
        height: u32,
        scale_float: f64,
        is_created: bool,
    },
    /// fractal scale handle
    PreferredScale { scale_u32: u32, scale_float: f64 },
}

impl From<DispatchMessageInner> for DispatchMessage {
    fn from(val: DispatchMessageInner) -> Self {
        match val {
            DispatchMessageInner::NewDisplay(_) => unimplemented!(),
            DispatchMessageInner::MouseButton {
                state,
                serial,
                button,
                time,
            } => DispatchMessage::MouseButton {
                state,
                serial,
                button,
                time,
            },
            DispatchMessageInner::MouseLeave => DispatchMessage::MouseLeave,
            DispatchMessageInner::MouseEnter {
                pointer,
                serial,
                surface_x,
                surface_y,
            } => DispatchMessage::MouseEnter {
                pointer,
                serial,
                surface_x,
                surface_y,
            },
            DispatchMessageInner::MouseMotion {
                time,
                surface_x,
                surface_y,
            } => DispatchMessage::MouseMotion {
                time,
                surface_x,
                surface_y,
            },
            DispatchMessageInner::TouchDown {
                serial,
                time,
                id,
                x,
                y,
            } => DispatchMessage::TouchDown {
                serial,
                time,
                id,
                x,
                y,
            },
            DispatchMessageInner::TouchUp {
                serial,
                time,
                id,
                x,
                y,
            } => DispatchMessage::TouchUp {
                serial,
                time,
                id,
                x,
                y,
            },
            DispatchMessageInner::TouchMotion { time, id, x, y } => {
                DispatchMessage::TouchMotion { time, id, x, y }
            }
            DispatchMessageInner::TouchCancel { id, x, y } => {
                DispatchMessage::TouchCancel { id, x, y }
            }
            DispatchMessageInner::RequestRefresh {
                width,
                height,
                scale_float,
                is_created,
            } => DispatchMessage::RequestRefresh {
                width,
                height,
                scale_float,
                is_created,
            },
            DispatchMessageInner::Axis {
                time,
                scale,
                horizontal,
                vertical,
                source,
            } => DispatchMessage::Axis {
                time,
                scale,
                horizontal,
                vertical,
                source,
            },
            DispatchMessageInner::ModifiersChanged(modifier) => {
                DispatchMessage::ModifiersChanged(modifier)
            }
            DispatchMessageInner::KeyboardInput {
                event,
                is_synthetic,
            } => DispatchMessage::KeyboardInput {
                event,
                is_synthetic,
            },
            DispatchMessageInner::PreferredScale {
                scale_u32,
                scale_float,
            } => DispatchMessage::PreferredScale {
                scale_u32,
                scale_float,
            },
            DispatchMessageInner::RefreshSurface { .. } => unimplemented!(),
            DispatchMessageInner::XdgInfoChanged(_) => unimplemented!(),
        }
    }
}
