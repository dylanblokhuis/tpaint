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

    let handle_keydown = move |event: Event<crate::vdom::events::KeyInput>| match event.key.name() {
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

        _ => {}
    };

    let handle_input = move |event: Event<crate::vdom::events::Text>| {
        // backspace and delete
        if event.0 == '\u{8}' || event.0 == '\u{7f}' {
            return;
        }

        println!("{:?}", event.0);

        let mut chars = text.borrow_mut().chars().collect::<Vec<_>>();
        chars.insert(*cursor_pos.get(), event.0);
        text.set(chars.iter().collect());
        cursor_pos.set(cursor_pos + 1);
    };

    let cursor = if *is_focused.get() && *cursor_visible.get() {
        *cursor_pos.get() as i64
    } else {
        -1
    };

    render! {
      view {
        class: "{cx.props.name} focus:border-2 border-1 border-gray-300 focus:border-black bg-white min-w-100 h-32 p-5 rounded-5 flex-col justify-center text-20",
        onkeydown: handle_keydown,
        oninput: handle_input,
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
