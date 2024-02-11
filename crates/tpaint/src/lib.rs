#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]

pub mod components;
mod dom;
mod event_loop;
pub mod events;
mod renderer;
mod tailwind;

pub use renderer::RendererDescriptor;

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

pub use event_loop::DomEventLoop;

pub mod prelude {
    pub use dioxus::prelude::*;

    #[cfg(feature = "hot-reload")]
    pub mod dioxus_hot_reload {
        pub use dioxus_hot_reload::*;
    }

    pub mod dioxus_elements {
        pub type AttributeDescription = (&'static str, Option<&'static str>, bool);

        pub struct view;
        impl view {
            pub const TAG_NAME: &'static str = "view";
            pub const NAME_SPACE: Option<&'static str> = None;
            pub const class: AttributeDescription = ("class", None, false);

            /// Prefix texture ids with ``texture://``
            pub const src: AttributeDescription = ("src", None, false);

            /// Will activate any classes prefixed with ``active:`` if this is true
            pub const is_active: AttributeDescription = ("is_active", None, false);

            pub const tabindex: AttributeDescription = ("tabindex", None, false);
            pub const text_cursor: AttributeDescription = ("text_cursor", None, false);
            pub const text_cursor_visible: AttributeDescription =
                ("text_cursor_visible", None, false);
            pub const text_selection_start: AttributeDescription =
                ("text_selection_start", None, false);
            pub const global_selection_mode: AttributeDescription =
                ("global_selection_mode", None, false);
        }

        pub mod events {

            impl_event! [
                crate::events::ClickEvent;
                onclick
                onmouseup
                onmousedown
            ];

            impl_event! [
                crate::events::MouseMoveEvent;
                onmousemove
            ];

            impl_event! [
                crate::events::InputEvent;
                oninput
            ];

            impl_event! [
                crate::events::KeyInput;
                onkeydown
                onkeyup
            ];

            impl_event! [
                crate::events::FocusEvent;
                onfocus
            ];

            impl_event! [
                crate::events::BlurEvent;
                onblur
            ];

            impl_event! [
                crate::events::DragEvent;
                ondrag
            ];

            impl_event![
                crate::events::LayoutEvent;
                onlayout
            ];

            impl_event![
                crate::events::SelectEvent;
                onselect
            ];
        }
    }
}

pub mod epaint {
    pub use epaint::*;
}
