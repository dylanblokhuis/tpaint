use dioxus::prelude::*;
use tpaint::components::Input;
use tpaint::hooks::{animation::Animation, use_animation};
use tpaint::prelude::*;

pub fn app(cx: Scope) -> Element {
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

    render! {
        view {
            class: "flex-col w-full h-full bg-red-200",



            Input {
                name: "input1",
            }

            view {
                class: "w-100 h-100 bg-red-500 hover:bg-red-900",
                onclick: move |_| {
                    println!("Clicked");
                },
                onmousemove: move |_| {
                    println!("Mouse moved");
                }
            }

            image {
                class: "w-200 rounded-full focus:rounded-100",
                src: "../../example_ui/assets/placeholder.png"
            }

            view {
                class: "w-100 h-100 bg-blue-500 hover:bg-blue-700 items-center justify-center",

                view {
                    class: "w-50 h-50 bg-green-500 hover:bg-green-700 flex-col items-center ",

                    view {
                        class: "w-25 h-25 bg-slate-500 hover:bg-slate-700",
                    }
                }
            }
            view {
                class: "text-20 text-red-500 bg-indigo-300",
                "Backend"
            }

            view {
                class: "focus:border-2 focus:border-black bg-white hover:bg-blue-300 w-140 h-100 ",

                view {
                    "sdfkljsd"
                }
            }

            Input {
                name: "input2",
            }

            // span {
            //     class: "text-20 text-black font-mono",
            //     "Mono font"
            // }
        }
    }
}
