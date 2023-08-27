// credits to https://github.com/marc2332/freya
use dioxus::prelude::*;
use std::time::Duration;
use tokio::time::interval;
use uuid::Uuid;

use super::animation::Animation;

/// Manage the lifecyle of an [Animation].
#[derive(Clone)]
pub struct AnimationManager<'a> {
    init_value: f64,
    current_animation_id: &'a UseState<Option<Uuid>>,
    value: &'a UseState<f64>,
    cx: &'a ScopeState,
}

impl<'a> AnimationManager<'a> {
    /// Start the given [Animation].
    pub fn start(&self, mut anim: Animation) {
        let new_id = Uuid::new_v4();
        let mut index = 0;

        let value = self.value.clone();
        let current_animation_id = self.current_animation_id.clone();

        // Set as current this new animation
        current_animation_id.set(Some(new_id));

        // Spawn the animation that will run at 1ms speed
        self.cx.spawn(async move {
            let mut ticker = interval(Duration::from_millis(1));
            loop {
                // Stop running the animation if it was removed
                if *current_animation_id.current() == Some(new_id) {
                    // Remove the current animation if it has finished
                    if anim.is_finished() {
                        current_animation_id.set(None);
                        break;
                    }

                    // Advance one tick
                    value.set(anim.move_value(index));
                    index += 1;

                    // Wait 1m
                    ticker.tick().await;
                } else {
                    break;
                }
            }
        });
    }

    /// Clear the currently running [Animation].
    pub fn clear(&self) {
        self.current_animation_id.set(None);
        self.set_value(self.init_value);
    }

    /// Check whether there is an [Animation] running or not.
    pub fn is_animating(&self) -> bool {
        self.current_animation_id.is_some()
    }

    /// Get the current value of the [Animation].
    pub fn value(&self) -> f64 {
        *self.value.current()
    }

    /// Set a new value for the [Animation].
    pub fn set_value(&self, new_value: f64) {
        self.value.set(new_value);
    }
}

/// Run animations.
///
/// ## Usage
/// ```rust
/// # use freya::prelude::*;
/// fn app(cx: Scope) -> Element {
///     let animation = use_animation(cx, 0.0);
///
///     let progress = animation.value();
///
///     use_effect(cx, (), move |_| {
///         animation.start(Animation::new_linear(0.0..=100.0, 50));
///         async move {}
///     });
///
///     render!(
///         rect {
///             width: "{progress}",
///         }
///     )
/// }
/// ```
///
pub fn use_animation(cx: &ScopeState, init_value: f64) -> AnimationManager {
    let current_animation_id = use_state(cx, || None);
    let value = use_state(cx, || init_value);

    AnimationManager {
        current_animation_id,
        value,
        cx,
        init_value,
    }
}
