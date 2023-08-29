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
            class: "flex-col gap-4",
            view {
                class: "w-100 h-100 bg-blue-500 hover:bg-blue-700"
            }

            view {
                class: "w-100 h-100 bg-red-500",
            }

            view {
                class: "w-{progress} h-{progress} bg-sky-500",
            }

            view {
                class: "w-100 h-100 rounded-{progress} bg-indigo-500",
            }

            // img {
            //     class: "w-200 rounded-{progress}",
            //     src: "../../example_ui/assets/placeholder.png"
            // }

            // span {
            //     class: "text-20 text-black",
            //     "Backend: {ctx.backend}"
            // }
            // span {
            //     class: "text-20 text-black font-mono",
            //     "Mono font"
            // }
        }
    })
}
