use std::borrow::BorrowMut;

use crate::prelude::*;
use dioxus::prelude::*;

#[derive(Props)]
pub struct InputProps<'a> {
    pub name: &'a str,
}

pub fn Input<'a>(cx: Scope<'a, InputProps<'a>>) -> Element {
    let mut text = use_state(cx, || "".to_string());
    let cursor_pos = use_state(cx, || 0);
    let is_focused = use_state(cx, || false);
    let cursor_visible = use_state(cx, || false);

    let cursor_blinking = use_future(
        cx,
        (cursor_visible, is_focused),
        |(cursor_visible, is_focused)| async move {
            if !*is_focused.get() {
                return;
            }

            loop {
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                cursor_visible.set(!*cursor_visible.get());
            }
        },
    );

    let handle_keydown = move |event: Event<crate::vdom::events::KeyInput>| {
        match event.key.name() {
            "Backspace" => {
                if *cursor_pos.get() > 0 {
                    let mut chars = text.borrow_mut().chars().collect::<Vec<_>>();
                    chars.remove(cursor_pos - 1);
                    text.set(chars.iter().collect());
                    cursor_pos.set(cursor_pos - 1);
                }
            }
            "Delete" => {
                if *cursor_pos.get() < text.borrow_mut().len() {
                    let mut chars = text.borrow_mut().chars().collect::<Vec<_>>();
                    chars.remove(*cursor_pos.get());
                    text.set(chars.iter().collect());
                }
            }
            "Left" => {
                if *cursor_pos.get() > 0 {
                    cursor_pos.set(cursor_pos - 1);
                }
            }
            "Right" => {
                if *cursor_pos.get() < text.borrow_mut().len() {
                    cursor_pos.set(cursor_pos + 1);
                }
            }
            "Space" => {
                let mut chars = text.borrow_mut().chars().collect::<Vec<_>>();
                chars.insert(*cursor_pos.get(), ' ');
                text.set(chars.iter().collect());
                cursor_pos.set(cursor_pos + 1);
            }
            key if key.len() == 1 => {
                // assuming single character input
                let mut chars = text.borrow_mut().chars().collect::<Vec<_>>();
                chars.insert(*cursor_pos.get(), key.chars().next().unwrap());
                text.set(chars.iter().collect());
                cursor_pos.set(cursor_pos + 1);
            }
            _ => {}
        }
    };

    let cursor = if *is_focused.get() && *cursor_visible.get() {
        *cursor_pos.get() as i64
    } else {
        -1
    };

    render! {
      view {
        class: "{cx.props.name} focus:border-2 focus:border-black bg-white hover:bg-blue-300 w-140 h-100",
        onkeydown: handle_keydown,
        onfocus: move |_| {
          cursor_blinking.cancel(cx);
          cursor_blinking.restart();
          is_focused.set(true);
        },
        onblur: move |_| {
          cursor_blinking.cancel(cx);
          is_focused.set(false);
        },
        cursor: cursor,

        "{text}"
      }
    }
}
