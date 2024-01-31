use std::default;

use dioxus::prelude::*;
use epaint::Rect;
use taffy::Layout;

use crate::prelude::{dioxus_elements::events::onlayout, *};

#[derive(PartialEq, Clone, Debug, Default)]
enum TransitionEasing {
    #[default]
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
    Custom,
}

#[derive(PartialEq, Clone, Debug)]
pub struct Transition {
    ease: TransitionEasing,
    duration: f32,
}

impl Default for Transition {
    fn default() -> Self {
        Self {
            ease: TransitionEasing::Linear,
            duration: 0.5,
        }
    }
}

// type AnimationGoal = String;

#[derive(PartialEq, Clone, Debug, Props)]
pub struct Props<'a> {
    class: &'a str,
    animate: String,
    transition: Transition,
}

#[derive(PartialEq, Clone, Debug)]
struct StylePiece {
    property: String,
    value: f32,
}

pub fn Motion<'a>(cx: Scope<'a, Props>) -> Element<'a> {
    // let class = use_state(cx, || cx.props.class);
    let current_rect = use_state(cx, || (Rect::ZERO, Layout::new()));

    use_effect(cx, (&cx.props.animate,), |(animate,)| {
        //
        to_owned![current_rect];
        async move {
            // extract all numbers that are prefixed by a '-', example: "w-400 h-400 left-100"
            let classes = animate.split_whitespace();
            let start_val = current_rect.get().clone();

            let height = start_val.1.size.height;
            for class in classes {
                // we do width, height, padding, border, margin, left, right, top, bottom
                if let Some(property) = class.strip_prefix("h-") {}
            }

            // we interpolate from the start value to the end value
            // we need to know the end value
        }
    });

    render! {
        view {
            class: "{cx.props.class}",
            onlayout: |event| {
                println!("layout: {:?}", event);
                current_rect.set((event.rect, event.layout));
            },

            "motion!"
        }
    }
}
