use dioxus::prelude::*;
use tpaint::{components::Input, prelude::*};

pub fn app(cx: Scope) -> Element {
    render! {
      view {
        class: "flex-col w-full p-10 bg-slate-200",

        view {
          class: "my-40",

          Input {

          }
        }


        example_ui::app {}
      }
    }
}
