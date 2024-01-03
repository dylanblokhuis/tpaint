use dioxus::prelude::*;
use tpaint::prelude::*;

pub fn app(cx: Scope) -> Element {
    render! {
        example_ui::app {}
    }
}
