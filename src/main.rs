#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]

use std::future::Future;

use dioxus::prelude::*;
use winit::{
    event,
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};

use crate::{
    hooks::{animation::Animation, use_animation},
    vdom::DomEventLoop,
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
        impl_event! [
            crate::vdom::events::PointerInput;
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

    let mut dom_event_loop =
        DomEventLoop::spawn(app, window.inner_size(), event_loop.create_proxy(), ());

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            winit::event::Event::WindowEvent { event, .. } => {
                let redraw = dom_event_loop.on_window_event(&event);

                if redraw {
                    window.request_redraw();
                }
            }
            winit::event::Event::UserEvent(_) => {
                window.request_redraw();
            }
            winit::event::Event::RedrawRequested(_) => {
                let primitives = dom_event_loop.get_paint_info();
                println!("primitives: {:?}", primitives);
                println!("\nredrawing!\n");
            }
            _ => (),
        }
    });
}

// struct Dom {
//     templates: FxHashMap<String, SmallVec<usize>>,
// }
