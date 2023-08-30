#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]

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
            #[allow(non_upper_case_globals)]
            pub const class: AttributeDescription = ("class", None, false);
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
        }
    }
}

pub mod epaint {
    pub use epaint::*;
}
