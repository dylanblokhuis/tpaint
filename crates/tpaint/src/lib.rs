#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]

pub mod components;
pub mod hooks;
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
    T: std::future::Future<Output = ()> + 'static,
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

pub use vdom::DomEventLoop;

pub mod prelude {
    pub mod dioxus_elements {
        pub type AttributeDescription = (&'static str, Option<&'static str>, bool);

        pub struct view;
        impl view {
            pub const TAG_NAME: &'static str = "view";
            pub const NAME_SPACE: Option<&'static str> = None;
            pub const class: AttributeDescription = ("class", None, false);
            pub const cursor: AttributeDescription = ("cursor", None, false);
            pub const cursor_visible: AttributeDescription = ("cursor_visible", None, false);
            pub const selection_start: AttributeDescription = ("selection_start", None, false);
        }

        #[cfg(feature = "images")]
        pub struct image;

        #[cfg(feature = "images")]
        impl image {
            pub const TAG_NAME: &'static str = "image";
            pub const NAME_SPACE: Option<&'static str> = None;
            pub const class: AttributeDescription = ("class", None, false);
            pub const src: AttributeDescription = ("src", None, false);
        }

        pub mod events {
            impl_event! [
                crate::vdom::events::PointerInput;
                onclick
                onmouseup
                onmousedown
            ];

            impl_event! [
                crate::vdom::events::PointerMove;
                onmousemove
            ];

            impl_event! [
                crate::vdom::events::Text;
                oninput
            ];

            impl_event! [
                crate::vdom::events::KeyInput;
                onkeydown
                onkeyup
            ];

            impl_event! [
                crate::vdom::events::Focus;
                onfocus
            ];

            impl_event! [
                crate::vdom::events::Blur;
                onblur
            ];

            impl_event! [
                crate::vdom::events::Drag;
                ondrag
            ];
        }
    }
}

pub mod epaint {
    pub use epaint::*;
}
