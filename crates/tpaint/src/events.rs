use std::{any::Any, rc::Rc, sync::Arc};

use dioxus::core::ElementId;


#[derive(Debug, Clone, PartialEq)]
pub struct Event;

impl Event {
    pub fn into_any(self) -> Rc<dyn Any> {
        match self {
            // Event::PointerInput(pointer_input) => Rc::new(pointer_input),
            // Event::PointerMoved(pointer_move) => Rc::new(pointer_move),
            // Event::Key(key_input) => Rc::new(key_input),
            // Event::Focus(focus) => Rc::new(focus),
            // Event::Blur(blur) => Rc::new(blur),
            // Event::Text(text) => Rc::new(text),
            // Event::Drag(drag) => Rc::new(drag),
            _ => unimplemented!(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DomEvent {
    pub name: Arc<str>,
    pub data: Arc<Event>,
    pub element_id: ElementId,
    pub bubbles: bool,
}
