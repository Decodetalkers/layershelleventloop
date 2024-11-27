use crate::reexport::{Anchor, Layer};
use iced::window::Id as IcedId;
use iced_core::mouse::Interaction;
use layershellev::id::Id as LayerId;
use layershellev::NewLayerShellSettings;

pub(crate) type LayerShellActionVec<T> = Vec<LayerShellAction<T>>;

#[derive(Debug, Clone)]
pub(crate) enum LayerShellAction<INFO: Clone> {
    Mouse(Interaction),
    CustomActions(LayershellCustomActionsWithInfo<INFO>),
    CustomActionsWithId(LayershellCustomActionsWithIdInner<INFO>),
    RedrawAll,
    RedrawWindow(LayerId), // maybe one day it is useful, but now useless
    NewMenu((IcedNewPopupSettings, INFO)),
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct IcedNewPopupSettings {
    pub size: (u32, u32),
    pub position: (i32, i32),
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum MenuDirection {
    Up,
    Down,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct IcedNewMenuSettings {
    pub size: (u32, u32),
    pub direction: MenuDirection,
}

#[derive(Debug, Clone)]
pub enum LayershellCustomActionsWithInfo<INFO: Clone> {
    AnchorChange(Anchor),
    LayerChange(Layer),
    AnchorSizeChange(Anchor, (u32, u32)),
    MarginChange((i32, i32, i32, i32)),
    SizeChange((u32, u32)),
    VirtualKeyboardPressed {
        time: u32,
        key: u32,
    },
    NewLayerShell((NewLayerShellSettings, INFO)),
    NewPopUp((IcedNewPopupSettings, INFO)),
    NewMenu((IcedNewMenuSettings, INFO)),
    /// is same with WindowAction::Close(id)
    RemoveWindow(IcedId),
    ForgetLastOutput,
}

pub type LayershellCustomActions = LayershellCustomActionsWithInfo<()>;

#[derive(Debug, Clone)]
pub struct LayershellCustomActionsWithIdAndInfo<INFO: Clone>(
    pub Option<IcedId>,
    pub LayershellCustomActionsWithInfo<INFO>,
);

impl<INFO: Clone> LayershellCustomActionsWithIdAndInfo<INFO> {
    pub fn new(id: Option<IcedId>, actions: LayershellCustomActionsWithInfo<INFO>) -> Self {
        Self(id, actions)
    }
}

pub type LayershellCustomActionsWithId = LayershellCustomActionsWithIdAndInfo<()>;

// first one means
#[derive(Debug, Clone)]
pub(crate) struct LayershellCustomActionsWithIdInner<INFO: Clone>(
    pub Option<LayerId>,                       // come from
    pub Option<LayerId>,                       // target if has one
    pub LayershellCustomActionsWithInfo<INFO>, // actions
);
