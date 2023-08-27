#![allow(non_snake_case)]

use dioxus::prelude::*;

use crate::hooks::{animation::Animation, use_animation};

mod hooks;
mod vdom;

#[derive(PartialEq, Props)]
struct YoProps {
    progress: f64,
}

fn main() {
    fn Yo(cx: Scope<YoProps>) -> Element {
        render!(
            main {
                h2 { "Yo: {cx.props.progress}" }
            }
        )
    }

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
            div {
                class: "w-{progress} h-200",
                h1 { "Hello World" }
                p { "This is a paragraph" }
                Yo {
                    progress: progress,
                }
                button {
                    onclick: move |_| {
                        println!("clicked");
                    },
                }
            }
        )
    }

    let mut vdom = VirtualDom::new(app);
    let mutations = vdom.rebuild();
    dbg!(&mutations);
    let mut render_vdom = vdom::VDom::new();
    render_vdom.apply_mutations(mutations);

    // render_vdom.traverse_tree(render_vdom.get_root_id(), &|node| {
    //     // println!("YOOO {:?}", node.tag);
    // });

    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async move {
            loop {
                render_vdom.traverse_tree(render_vdom.get_root_id(), &|node| {
                    println!("YOOO {:?}", node.tag);
                });
                vdom.wait_for_work().await;

                let mutations = vdom.render_immediate();
                // dbg!(&mutations);
                render_vdom.apply_mutations(mutations);

                // dbg!(render_vdom.nodes.len());
            }
        });
}

// struct Dom {
//     templates: FxHashMap<String, SmallVec<usize>>,
// }
