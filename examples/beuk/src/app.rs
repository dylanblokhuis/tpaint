use dioxus::prelude::*;
use tpaint::prelude::*;

pub fn app(cx: Scope) -> Element {
    render! {
      view {
        class: "flex-col w-full p-10 bg-slate-200",

        example_ui::app {}

        // view {
        //   class: "w-200 h-200 bg-rose-900 shrink-0",
        // }


        // view {
        //     class: "bg-red-400 w-full flex-col grow-0",
        //     "Lorem ipsum dolor sit amet, consectetur adipiscing elit. Vivamus ultrices vel urna nec dignissim. Nam ultrices elit id leo blandit sollicitudin. Phasellus dapibus augue ut augue condimentum, suscipit rhoncus ante elementum. Donec ut ante vel leo sodales iaculis in id neque. Praesent finibus risus egestas nisl fermentum, sit amet porttitor erat finibus. Aliquam nibh turpis, bibendum ut quam viverra, ullamcorper rutrum ex. Mauris arcu purus, venenatis vitae accumsan vitae, placerat id dolor. Mauris suscipit interdum lectus, ut ornare enim semper id. Sed et tempus nibh, vitae condimentum tortor. Quisque quis leo at sapien rutrum fermentum. Morbi iaculis, dui eleifend euismod malesuada, ligula ex semper velit, sit amet facilisis enim massa ut lacus. Ut sagittis tellus non sapien ornare feugiat. Curabitur a pretium massa. Integer pharetra risus vel quam mattis porta. Etiam suscipit rutrum cursus. Mauris aliquam ut ipsum et tincidunt.",

        //     view {
        //       class: "w-full p-10 bg-blue-900",

        //       "Lorem ipsum dolor sit amet, consectetur adipiscing elit. Vivamus ultrices vel urna nec dignissim. Nam ultrices elit id leo blandit sollicitudin. Phasellus dapibus augue ut augue condimentum, suscipit rhoncus ante elementum. Donec ut ante vel leo sodales iaculis in id neque. Praesent finibus risus egestas nisl fermentum, sit amet porttitor erat finibus. Aliquam nibh turpis, bibendum ut quam viverra, ullamcorper rutrum ex. Mauris arcu purus, venenatis vitae accumsan vitae, placerat id dolor. Mauris suscipit interdum lectus, ut ornare enim semper id. Sed et tempus nibh, vitae condimentum tortor. Quisque quis leo at sapien rutrum fermentum. Morbi iaculis, dui eleifend euismod malesuada, ligula ex semper velit, sit amet facilisis enim massa ut lacus. Ut sagittis tellus non sapien ornare feugiat. Curabitur a pretium massa. Integer pharetra risus vel quam mattis porta. Etiam suscipit rutrum cursus. Mauris aliquam ut ipsum et tincidunt.",
        //     }
        // }

        // view {
        //   class: "w-200 h-200 bg-indigo-900 shrink-0",
        // }

        // "Hello, world!",

        // view {
        //   class: "w-200 h-200 bg-red-500",

        //   "Yo"
        // }

        // (0..5).map(|_| rsx! {
        //     view {
        //         class: "w-full h-50 p-10 bg-blue-900",
        //     }
        // })
      }
    }
}
