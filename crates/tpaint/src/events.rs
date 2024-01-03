use std::{any::Any, rc::Rc, sync::Arc};

use dioxus::core::ElementId;


use winit::event::{ModifiersState, MouseButton};

use crate::dom::{DomState};

#[derive(Debug, Clone)]
pub enum Event {
    Focus(FocusEvent),
    Blur(BlurEvent),
    Drag(DragEvent),
    Input(InputEvent),
    Key(KeyInput),
    Click(ClickEvent),
    MouseMove(MouseMoveEvent),
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
            _ => unimplemented!(),
        }
    }
}

impl DomState {
    pub fn modifiers(&self) -> ModifiersState {
        self.keyboard_state.modifiers
    }

    pub fn text_cursor(&self) -> usize {
        self.focused.unwrap().text_cursor.unwrap()
    }

    pub fn command(&self) -> bool {
        // on macos check logo key
        if cfg!(target_os = "macos") {
            return self.modifiers().logo();
        }

        // on windows and linux check control
        self.modifiers().ctrl()
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
    pub state: DomState,
}

#[derive(Clone, Debug)]
pub struct BlurEvent {
    pub state: DomState,
}

#[derive(Clone, Debug)]
pub struct DragEvent {
    pub state: DomState,
}

#[derive(Clone, Debug)]
pub struct InputEvent {
    pub state: DomState,
    pub text: char,
}

#[derive(Clone, Debug)]
pub struct KeyInput {
    pub state: DomState,
    pub key: winit::event::VirtualKeyCode,
    pub pressed: bool,
}

#[derive(Clone, Debug)]
pub struct ClickEvent {
    pub state: DomState,
    pub button: MouseButton,
    pub pressed: bool,
}

#[derive(Clone, Debug)]
pub struct MouseMoveEvent {
    pub state: DomState,
}
