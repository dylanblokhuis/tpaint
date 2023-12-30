use dioxus::prelude::*;
use tpaint::prelude::*;

pub fn app(cx: Scope) -> Element {
    render! {
      view {
        class: "flex-col w-full p-10 bg-slate-200",

        example_ui::app {}
      }
    }
}
