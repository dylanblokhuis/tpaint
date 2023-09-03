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
            class: "flex-col w-full h-full bg-red-200 p-10",

            Input {
                name: "input1",
            }

            view {
                class: "w-100 h-100 bg-red-500 hover:bg-red-900 mt-10",
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
                "Lorem ipsum dolor sit amet, consectetur adipiscing elit. Vivamus ultrices vel urna nec dignissim. Nam ultrices elit id leo blandit sollicitudin. Phasellus dapibus augue ut augue condimentum, suscipit rhoncus ante elementum. Donec ut ante vel leo sodales iaculis in id neque. Praesent finibus risus egestas nisl fermentum, sit amet porttitor erat finibus. Aliquam nibh turpis, bibendum ut quam viverra, ullamcorper rutrum ex. Mauris arcu purus, venenatis vitae accumsan vitae, placerat id dolor. Mauris suscipit interdum lectus, ut ornare enim semper id. Sed et tempus nibh, vitae condimentum tortor. Quisque quis leo at sapien rutrum fermentum. Morbi iaculis, dui eleifend euismod malesuada, ligula ex semper velit, sit amet facilisis enim massa ut lacus. Ut sagittis tellus non sapien ornare feugiat. Curabitur a pretium massa. Integer pharetra risus vel quam mattis porta. Etiam suscipit rutrum cursus. Mauris aliquam ut ipsum et tincidunt."
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
