use dioxus::prelude::*;
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

    cx.render(rsx! {
        view {
            class: "flex-col w-full h-full bg-red-200",

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
                class: "w-100 h-100 bg-red-500 hover:bg-red-900",
                onclick: move |_| {
                    println!("Clicked");
                },
                onmousemove: move |_| {
                    println!("Mouse moved");
                }
            }

            view {
                class: "w-{progress} h-{progress} bg-sky-500",
            }

            view {
                class: "w-100 h-100 rounded-{progress} bg-indigo-500",
            }

            image {
                class: "w-200 rounded-full",
                src: "../../example_ui/assets/placeholder.png"
            }

            view {
                class: "text-20 text-red-500 bg-indigo-300",
                "Backend"
            }
            // span {
            //     class: "text-20 text-black font-mono",
            //     "Mono font"
            // }
        }
    })
}
