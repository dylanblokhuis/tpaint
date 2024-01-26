use std::default;

use dioxus::prelude::*;

use crate::prelude::*;

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

    use_effect(cx, (&cx.props.animate,), |(animate,)| {
        //
        async move {
            // extract all numbers that are prefixed by a '-', example: "w-400 h-400 left-100"
            let words = animate.split_whitespace();

            // Initialize a vector to hold the extracted numbers
            let mut style_pieces = Vec::new();

            // Iterate through each word
            for word in words {
                // Check if the word contains a '-' followed by digits
                if let Some(index) = word.find('-') {
                    // Extract the substring after the '-'
                    let number_str = &word[index + 1..];

                    // Parse the substring into a number and add it to the vector
                    if let Ok(number) = number_str.parse::<f32>() {
                        style_pieces.push(StylePiece {
                            property: word[..index].to_string(),
                            value: number,
                        });
                    }
                }
            }

            // hmm this is hard to interpolate, we need the size of the element?
            println!("style_pieces: {:?}", style_pieces);
        }
    });

    render! {
        view {
            class: "{cx.props.class}",

            "motion!"
        }
    }
}
