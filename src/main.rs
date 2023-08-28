#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]

use dioxus::prelude::*;

use crate::hooks::{animation::Animation, use_animation};

mod hooks;
mod vdom;

mod dioxus_elements {
    pub type AttributeDescription = (&'static str, Option<&'static str>, bool);

    pub struct view;

    impl view {
        pub const TAG_NAME: &'static str = "view";
        pub const NAME_SPACE: Option<&'static str> = None;
        #[allow(non_upper_case_globals)]
        pub const class: AttributeDescription = ("class", None, false);
    }
}

#[derive(PartialEq, Props)]
struct YoProps {
    progress: f64,
}

fn main() {
    fn app(cx: Scope) -> Element {
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

        render!(
            view {
                class: "w-{progress} h-200 bg-red-500",

                view {}
                view {}
                view {}
                view {}
                // h1 { "Hello World" }
                // p { "This is a paragraph" }
                // Yo {
                //     progress: progress,
                // }
                // button {
                //     onclick: move |_| {
                //         println!("clicked");
                //     },
                // }
            }
        )
    }

    let mut vdom = VirtualDom::new(app);
    let mutations = vdom.rebuild();
    dbg!(&mutations);
    let mut render_vdom = vdom::VDom::new();
    render_vdom.apply_mutations(mutations);

  
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async move {
            loop {
                render_vdom.traverse_tree(render_vdom.get_root_id(), &|node| {                    
                    // println!("{:?} {:?}", node.tag, node.attrs.get("class"));
                });
                vdom.wait_for_work().await;

                let mutations = vdom.render_immediate();
                render_vdom.apply_mutations(mutations);
            }
        });
}

// struct Dom {
//     templates: FxHashMap<String, SmallVec<usize>>,
// }
