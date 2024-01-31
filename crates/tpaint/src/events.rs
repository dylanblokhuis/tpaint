use std::{any::Any, rc::Rc, sync::Arc};

use dioxus::core::ElementId;

use epaint::text::cursor::Cursor;
use taffy::{Layout, NodeId};
use winit::{
    event::{ElementState, Modifiers, MouseButton},
    keyboard::{Key, PhysicalKey, SmolStr},
};

use crate::dom::{Dom, DomState};

#[derive(Debug, Clone)]
pub enum Event {
    Focus(FocusEvent),
    Blur(BlurEvent),
    Drag(DragEvent),
    Input(InputEvent),
    Key(KeyInput),
    Click(ClickEvent),
    MouseMove(MouseMoveEvent),
    Layout(LayoutEvent),
    Select(SelectEvent),
}

impl Event {
    pub fn into_any(self) -> Rc<dyn Any> {
        match self {
            Event::Focus(focus) => Rc::new(focus),
            Event::Blur(blur) => Rc::new(blur),
            Event::Drag(drag) => Rc::new(drag),
            Event::Input(input) => Rc::new(input),
            Event::Key(key_input) => Rc::new(key_input),
            Event::Click(click) => Rc::new(click),
            Event::MouseMove(mouse_move) => Rc::new(mouse_move),
            Event::Layout(layout) => Rc::new(layout),
            Event::Select(select) => Rc::new(select),
        }
    }
}

impl DomState {
    pub fn modifiers(&self) -> Modifiers {
        self.keyboard_state.modifiers
    }

    pub fn command(&self) -> bool {
        // on macos check logo key
        if cfg!(target_os = "macos") {
            return self.modifiers().state().super_key();
        }

        // on windows and linux check control

        self.modifiers().state().control_key()
    }

    pub fn shift(&self) -> bool {
        self.modifiers().state().shift_key()
    }
}

#[derive(Clone, Debug)]
pub struct EventState {
    dom_state: DomState,
}

impl EventState {
    pub fn new(dom: &Dom, node_id: NodeId) -> Self {
        // let rect = dom.tree.get_node_context(node_id).unwrap().computed.rect;
        Self {
            // rect,
            dom_state: dom.state.clone(),
        }
    }

    pub fn state(&self) -> &DomState {
        &self.dom_state
    }
}

#[derive(Debug, Clone)]
pub struct DomEvent {
    pub name: Arc<str>,
    pub data: Arc<Event>,
    pub element_id: ElementId,
    pub bubbles: bool,
}

#[derive(Clone, Debug)]
pub struct FocusEvent {
    pub state: EventState,
}

#[derive(Clone, Debug)]
pub struct BlurEvent {
    pub state: EventState,
}

#[derive(Clone, Debug)]
pub struct DragEvent {
    pub state: EventState,
}

#[derive(Clone, Debug)]
pub struct InputEvent {
    pub state: EventState,
    pub logical_key: Key,
    pub physical_key: PhysicalKey,
    pub text: Option<SmolStr>,
}

#[derive(Clone, Debug)]
pub struct KeyInput {
    pub state: EventState,
    pub element_state: ElementState,
    pub logical_key: Key,
    pub physical_key: PhysicalKey,
    pub text: Option<SmolStr>,
}

#[derive(Clone, Debug)]
pub struct ClickEvent {
    pub state: EventState,
    pub button: MouseButton,
    pub element_state: ElementState,
    pub text_cursor_position: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct MouseMoveEvent {
    pub state: EventState,
}

#[derive(Clone, Debug)]
pub struct LayoutEvent {
    pub state: EventState,
    /// The absolute position of the element.
    pub rect: epaint::Rect,
    /// Computed style of the element.
    pub layout: Layout,
}

#[derive(Clone, Debug)]
pub struct SelectEvent {
    pub state: EventState,
    pub start_cursor: Cursor,
    pub end_cursor: Cursor,
}
