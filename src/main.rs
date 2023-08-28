#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]

use std::{future::Future, sync::Arc};

use dioxus::prelude::*;
use tao::{
    event::WindowEvent,
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};

use crate::{
    dioxus_elements::events::MouseData,
    hooks::{animation::Animation, use_animation},
    vdom::{DomEvent, DomEventLoop},
};

mod hooks;
mod vdom;

#[doc(hidden)]
pub trait EventReturn<P>: Sized {
    fn spawn(self, _cx: &dioxus::core::ScopeState) {}
}

impl EventReturn<()> for () {}
#[doc(hidden)]
pub struct AsyncMarker;

impl<T> EventReturn<AsyncMarker> for T
where
    T: Future<Output = ()> + 'static,
{
    #[inline]
    fn spawn(self, cx: &dioxus::core::ScopeState) {
        cx.spawn(self);
    }
}

macro_rules! impl_event {
    (
        $data:ty;
        $(
            $( #[$attr:meta] )*
            $name:ident
        )*
    ) => {
        $(
            $( #[$attr] )*
            #[inline]
            pub fn $name<'a, E: crate::EventReturn<T>, T>(_cx: &'a dioxus::core::ScopeState, mut _f: impl FnMut(dioxus::core::Event<$data>) -> E + 'a) -> dioxus::core::Attribute<'a> {
                dioxus::core::Attribute::new(
                    stringify!($name),
                    _cx.listener(move |e: dioxus::core::Event<$data>| {
                        _f(e).spawn(_cx);
                    }),
                    None,
                    false,
                )
            }
        )*
    };
}

mod dioxus_elements {
    pub type AttributeDescription = (&'static str, Option<&'static str>, bool);

    pub struct view;

    impl view {
        pub const TAG_NAME: &'static str = "view";
        pub const NAME_SPACE: Option<&'static str> = None;
        #[allow(non_upper_case_globals)]
        pub const class: AttributeDescription = ("class", None, false);
    }

    pub mod events {
        #[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
        #[derive(Clone, Default, PartialEq, Eq, Debug)]
        /// Data associated with a mouse event
        ///
        /// Do not use the deprecated fields; they may change or become private in the future.
        pub struct MouseData {
            /// True if the alt key was down when the mouse event was fired.
            #[deprecated(since = "0.3.0", note = "use modifiers() instead")]
            pub alt_key: bool,

            /// The button number that was pressed (if applicable) when the mouse event was fired.
            #[deprecated(since = "0.3.0", note = "use trigger_button() instead")]
            pub button: i16,

            /// Indicates which buttons are pressed on the mouse (or other input device) when a mouse event is triggered.
            ///
            /// Each button that can be pressed is represented by a given number (see below). If more than one button is pressed, the button values are added together to produce a new number. For example, if the secondary (2) and auxiliary (4) buttons are pressed simultaneously, the value is 6 (i.e., 2 + 4).
            ///
            /// - 1: Primary button (usually the left button)
            /// - 2: Secondary button (usually the right button)
            /// - 4: Auxiliary button (usually the mouse wheel button or middle button)
            /// - 8: 4th button (typically the "Browser Back" button)
            /// - 16 : 5th button (typically the "Browser Forward" button)
            #[deprecated(since = "0.3.0", note = "use held_buttons() instead")]
            pub buttons: u16,

            /// The horizontal coordinate within the application's viewport at which the event occurred (as opposed to the coordinate within the page).
            ///
            /// For example, clicking on the left edge of the viewport will always result in a mouse event with a clientX value of 0, regardless of whether the page is scrolled horizontally.
            #[deprecated(since = "0.3.0", note = "use client_coordinates() instead")]
            pub client_x: i32,

            /// The vertical coordinate within the application's viewport at which the event occurred (as opposed to the coordinate within the page).
            ///
            /// For example, clicking on the top edge of the viewport will always result in a mouse event with a clientY value of 0, regardless of whether the page is scrolled vertically.
            #[deprecated(since = "0.3.0", note = "use client_coordinates() instead")]
            pub client_y: i32,

            /// True if the control key was down when the mouse event was fired.
            #[deprecated(since = "0.3.0", note = "use modifiers() instead")]
            pub ctrl_key: bool,

            /// True if the meta key was down when the mouse event was fired.
            #[deprecated(since = "0.3.0", note = "use modifiers() instead")]
            pub meta_key: bool,

            /// The offset in the X coordinate of the mouse pointer between that event and the padding edge of the target node.
            #[deprecated(since = "0.3.0", note = "use element_coordinates() instead")]
            pub offset_x: i32,

            /// The offset in the Y coordinate of the mouse pointer between that event and the padding edge of the target node.
            #[deprecated(since = "0.3.0", note = "use element_coordinates() instead")]
            pub offset_y: i32,

            /// The X (horizontal) coordinate (in pixels) of the mouse, relative to the left edge of the entire document. This includes any portion of the document not currently visible.
            ///
            /// Being based on the edge of the document as it is, this property takes into account any horizontal scrolling of the page. For example, if the page is scrolled such that 200 pixels of the left side of the document are scrolled out of view, and the mouse is clicked 100 pixels inward from the left edge of the view, the value returned by pageX will be 300.
            #[deprecated(since = "0.3.0", note = "use page_coordinates() instead")]
            pub page_x: i32,

            /// The Y (vertical) coordinate in pixels of the event relative to the whole document.
            ///
            /// See `page_x`.
            #[deprecated(since = "0.3.0", note = "use page_coordinates() instead")]
            pub page_y: i32,

            /// The X coordinate of the mouse pointer in global (screen) coordinates.
            #[deprecated(since = "0.3.0", note = "use screen_coordinates() instead")]
            pub screen_x: i32,

            /// The Y coordinate of the mouse pointer in global (screen) coordinates.
            #[deprecated(since = "0.3.0", note = "use screen_coordinates() instead")]
            pub screen_y: i32,

            /// True if the shift key was down when the mouse event was fired.
            #[deprecated(since = "0.3.0", note = "use modifiers() instead")]
            pub shift_key: bool,
        }

        impl_event! [
            MouseData;
            onclick
        ];
    }
}

#[derive(PartialEq, Props)]
struct YoProps {
    progress: f64,
}

fn main() {
    std::env::set_var("RUSTLOG", "debug");
    simple_logger::SimpleLogger::new().init().unwrap();

    fn app(cx: Scope) -> Element {
        let animation = use_animation(cx, 0.0);
        let progress = animation.value();

        use_effect(cx, (&progress,), move |(val,)| {
            if val == 100.0 {
                animation.start(Animation::new_linear(100.0..=0.0, 1000));
            }

            if val == 0.0 {
                animation.start(Animation::new_linear(0.0..=100.0, 1000));
            }
            async move {}
        });

        render!(view {
            class: "w-{progress} h-200 bg-red-500",
            onclick: move |_| {
                println!("clicked!!!");
            },
            // h1 { "Hello World" }
            // p { "This is a paragraph" }
            // Yo {
            //     progress: progress,
            // }
            // button {
            //     onclick: move |_| {
            //         println!("clicked");
            //     },
            // }
        })
    }

    let event_loop = EventLoop::new();

    let window = WindowBuilder::new()
        .with_title("tpaint")
        .build(&event_loop)
        .unwrap();

    let dom_event_loop = DomEventLoop::spawn(app);

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            tao::event::Event::WindowEvent { event, .. } => match event {
                WindowEvent::CloseRequested => *control_flow = ControlFlow::Exit,
                WindowEvent::MouseInput {
                    device_id,
                    state,
                    button,
                    modifiers,
                } => {
                    println!("mouse input");
                    dom_event_loop
                        .dom_event_sender
                        .send(DomEvent {
                            element_id: dioxus::core::ElementId(1),
                            bubbles: true,
                            name: "click",
                            data: Arc::new(vdom::EventData::MouseData(MouseData {
                                ..Default::default()
                            })),
                        })
                        .unwrap();
                }
                _ => (),
            },
            tao::event::Event::RedrawRequested(_) => {
                // let triangles = ..
                println!("\nredrawing!\n");
            }
            _ => (),
        }
    });
}

// struct Dom {
//     templates: FxHashMap<String, SmallVec<usize>>,
// }
