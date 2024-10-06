use iced::mouse;
use layershellev::id::Id;
use layershellev::keyboard::ModifiersState;
use layershellev::reexport::wayland_client::{ButtonState, KeyState, WEnum};
use layershellev::xkb_keyboard::KeyEvent as LayerShellKeyEvent;
use layershellev::{DispatchMessage, WindowWrapper};

use iced_core::keyboard::Modifiers as IcedModifiers;

use crate::actions::IcedNewMenuSettings;

fn from_u32_to_icedmouse(code: u32) -> mouse::Button {
    match code {
        273 => mouse::Button::Right,
        _ => mouse::Button::Left,
    }
}
#[derive(Debug, Clone, Copy)]
pub enum IcedButtonState {
    Pressed(mouse::Button),
    Released(mouse::Button),
}

#[derive(Debug, Clone, Copy)]
pub enum IcedKeyState {
    Pressed,
    Released,
}

impl From<WEnum<KeyState>> for IcedKeyState {
    fn from(value: WEnum<KeyState>) -> Self {
        match value {
            WEnum::Value(KeyState::Released) => Self::Released,
            WEnum::Value(KeyState::Pressed) => Self::Pressed,
            _ => unreachable!(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum WindowEvent {
    ScaleFactorChanged {
        scale_u32: u32,
        scale_float: f64,
    },
    CursorEnter {
        x: f64,
        y: f64,
    },
    CursorMoved {
        x: f64,
        y: f64,
    },
    CursorLeft,
    MouseInput(IcedButtonState),
    Keyboard {
        state: IcedKeyState,
        key: u32,
        modifiers: IcedModifiers,
    },
    KeyBoardInput {
        event: LayerShellKeyEvent,
        is_synthetic: bool,
    },
    ModifiersChanged(ModifiersState),
    Axis {
        x: f32,
        y: f32,
    },
    PixelDelta {
        x: f32,
        y: f32,
    },
    TouchDown {
        id: i32,
        x: f64,
        y: f64,
    },
    TouchUp {
        id: i32,
        x: f64,
        y: f64,
    },
    TouchMotion {
        id: i32,
        x: f64,
        y: f64,
    },
    TouchCancel {
        id: i32,
        x: f64,
        y: f64,
    },
}

#[derive(Debug)]
pub enum IcedLayerEvent<Message: 'static, INFO: Clone> {
    RequestRefreshWithWrapper {
        width: u32,
        height: u32,
        fractal_scale: f64,
        wrapper: WindowWrapper,
        is_created: bool,
        info: Option<INFO>,
    },
    RequestRefresh {
        width: u32,
        height: u32,
        fractal_scale: f64,
    },
    Window(WindowEvent),
    NormalUpdate,
    UserEvent(Message),
    WindowRemoved(iced_core::window::Id),
    NewMenu((IcedNewMenuSettings, INFO)),
}

#[allow(unused)]
#[derive(Debug)]
pub struct MultiWindowIcedLayerEvent<Message: 'static, INFO: Clone>(
    pub Option<Id>,
    pub IcedLayerEvent<Message, INFO>,
);

impl<Message: 'static, INFO: Clone> From<(Option<Id>, IcedLayerEvent<Message, INFO>)>
    for MultiWindowIcedLayerEvent<Message, INFO>
{
    fn from((id, message): (Option<Id>, IcedLayerEvent<Message, INFO>)) -> Self {
        MultiWindowIcedLayerEvent(id, message)
    }
}

impl<Message: 'static, INFO: Clone> From<&DispatchMessage> for IcedLayerEvent<Message, INFO> {
    fn from(value: &DispatchMessage) -> Self {
        match value {
            DispatchMessage::RequestRefresh {
                width,
                height,
                scale_float,
                ..
            } => IcedLayerEvent::RequestRefresh {
                width: *width,
                height: *height,
                fractal_scale: *scale_float,
            },
            DispatchMessage::MouseEnter {
                surface_x: x,
                surface_y: y,
                ..
            } => IcedLayerEvent::Window(WindowEvent::CursorEnter { x: *x, y: *y }),
            DispatchMessage::MouseMotion {
                surface_x: x,
                surface_y: y,
                ..
            } => IcedLayerEvent::Window(WindowEvent::CursorMoved { x: *x, y: *y }),
            DispatchMessage::MouseLeave => IcedLayerEvent::Window(WindowEvent::CursorLeft),
            DispatchMessage::MouseButton { state, button, .. } => {
                let btn = from_u32_to_icedmouse(*button);
                match state {
                    WEnum::Value(ButtonState::Pressed) => IcedLayerEvent::Window(
                        WindowEvent::MouseInput(IcedButtonState::Pressed(btn)),
                    ),
                    WEnum::Value(ButtonState::Released) => IcedLayerEvent::Window(
                        WindowEvent::MouseInput(IcedButtonState::Released(btn)),
                    ),
                    _ => unreachable!(),
                }
            }
            DispatchMessage::TouchUp { id, x, y, .. } => {
                IcedLayerEvent::Window(WindowEvent::TouchUp {
                    id: *id,
                    x: *x,
                    y: *y,
                })
            }
            DispatchMessage::TouchDown { id, x, y, .. } => {
                IcedLayerEvent::Window(WindowEvent::TouchDown {
                    id: *id,
                    x: *x,
                    y: *y,
                })
            }
            DispatchMessage::TouchMotion { id, x, y, .. } => {
                IcedLayerEvent::Window(WindowEvent::TouchMotion {
                    id: *id,
                    x: *x,
                    y: *y,
                })
            }
            DispatchMessage::TouchCancel { id, x, y, .. } => {
                IcedLayerEvent::Window(WindowEvent::TouchCancel {
                    id: *id,
                    x: *x,
                    y: *y,
                })
            }
            DispatchMessage::PreferredScale {
                scale_u32,
                scale_float,
            } => IcedLayerEvent::Window(WindowEvent::ScaleFactorChanged {
                scale_u32: *scale_u32,
                scale_float: *scale_float,
            }),

            DispatchMessage::KeyboardInput {
                event,
                is_synthetic,
            } => IcedLayerEvent::Window(WindowEvent::KeyBoardInput {
                event: event.clone(),
                is_synthetic: *is_synthetic,
            }),
            DispatchMessage::ModifiersChanged(modifiers) => {
                IcedLayerEvent::Window(WindowEvent::ModifiersChanged(*modifiers))
            }
            DispatchMessage::Axis {
                horizontal,
                vertical,
                ..
            } => {
                if horizontal.stop && vertical.stop {
                    return Self::NormalUpdate;
                }
                let has_scroll = vertical.discrete != 0 || horizontal.discrete != 0;
                if has_scroll {
                    return IcedLayerEvent::Window(WindowEvent::Axis {
                        x: -horizontal.discrete as f32,
                        y: -vertical.discrete as f32,
                    });
                }
                IcedLayerEvent::Window(WindowEvent::Axis {
                    x: -horizontal.absolute as f32,
                    y: -vertical.absolute as f32,
                })
            }
        }
    }
}
