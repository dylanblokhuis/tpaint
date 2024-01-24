use dioxus::prelude::*;
use tpaint::{
    components::{image::Image, input::Input},
    prelude::*,
};

pub fn app(cx: Scope) -> Element {
    render! {
        view {
            class: "flex-col items-start gap-y-10 p-8 w-full overflow-y-scroll scrollbar-default bg-white",



            view {
                "Hey"
            }

            Input {}



            // view {
            //     class: "p-10 gap-x-40 shrink-0 flex-col",

            //     view {
            //         class: "w-200 h-200 p-15 flex-nowrap bg-rose-900 overflow-y-scroll overflow-x-scroll scrollbar-default",

            //         view {
            //             class: "w-150 h-100 bg-blue-300 focus:bg-indigo-800 shrink-0 ",
            //         }
            //         view {
            //             class: "w-300 h-300 shrink-0 bg-rose-500",
            //         }
            //     }

            //     view {
            //         class: "grow-0 flex-col w-full bg-red-500 active:bg-blue-600",
            //         is_active: true,

            //         "Lorem ipsum dolor sit amet, consectetur adipiscing elit. Vivamus ultrices vel urna nec dignissim. Nam ultrices elit id leo blandit sollicitudin. Phasellus dapibus augue ut augue condimentum, suscipit rhoncus ante elementum. Donec ut ante vel leo sodales iaculis in id neque. Praesent finibus risus egestas nisl fermentum, sit amet porttitor erat finibus. Aliquam nibh turpis, bibendum ut quam viverra, ullamcorper rutrum ex. Mauris arcu purus, venenatis vitae accumsan vitae, placerat id dolor. Mauris suscipit interdum lectus, ut ornare enim semper id. Sed et tempus nibh, vitae condimentum tortor. Quisque quis leo at sapien rutrum fermentum. Morbi iaculis, dui eleifend euismod malesuada, ligula ex semper velit, sit amet facilisis enim massa ut lacus. Ut sagittis tellus non sapien ornare feugiat. Curabitur a pretium massa. Integer pharetra risus vel quam mattis porta. Etiam suscipit rutrum cursus. Mauris aliquam ut ipsum et tincidunt."
            //     }
            // }


            // view {
            //     class: "w-200 h-200 bg-black p-10",

            //     Image {
            //         src: "https://placehold.co/600x400/png".to_string(),
            //     }
            // }

            // view {
            //     class: "bg-red-900 p-10",
            //     Image {
            //         src: "https://placehold.co/600x400".to_string(),
            //     }
            // }




            (0..10).map(|_| rsx! {
                view {
                    class: "w-full p-20 bg-blue-900",
                    "Lorem ipsum dolor sit amet"
                }
            })
        }
    }
}
