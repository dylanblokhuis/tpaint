use std::rc::Rc;

use dioxus::prelude::*;
use tpaint::{components::Input, prelude::*};

pub fn app(cx: Scope) -> Element {
    let some_data = use_state(cx, || 5);

    render! {
      view {
        class: "flex-col w-full p-10 bg-slate-200",

        view {
          class: "w-50 h-50 bg-red-500 items-center justify-center",
          onclick: move |_| {
            println!("Clicked");
            some_data.set(some_data.get() + 1);
          },

          "Click me {some_data}"
        },

        view {
          class: "my-40",

          Input {

          }

          Input {
            value: "{some_data}",
            onchange: move |value: Rc<String>| {
              println!("Here!: {}", value);
              some_data.set(value.parse::<i32>().unwrap_or(0));
            }
          }
        }




        example_ui::app {}
      }
    }
}
