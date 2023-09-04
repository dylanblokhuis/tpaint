use crate::prelude::*;
use copypasta::{ClipboardContext, ClipboardProvider};
use dioxus::prelude::*;

#[derive(Props)]
pub struct InputProps<'a> {
    pub name: &'a str,
}

pub fn Input<'a>(cx: Scope<'a, InputProps<'a>>) -> Element {
    let text = use_state(cx, || "".to_string());
    let cursor_pos = use_state(cx, || 0);
    let is_focused = use_state(cx, || false);
    let cursor_visible = use_state(cx, || false);
    let selection_start = use_state(cx, || 0);

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

    let get_text_range = || {
        let start = std::cmp::min(*selection_start.get(), *cursor_pos.get());
        let end = std::cmp::max(*selection_start.get(), *cursor_pos.get());
        let mut chars = text.chars().collect::<Vec<_>>();
        (chars.drain(start..end).collect::<String>(), start, end)
    };

    let is_all_selected = selection_start.get() == cursor_pos.get();

    
    let handle_keydown = move |event: Event<crate::vdom::events::KeyInput>| match event.key.name() {
        "Backspace" => {
            if *cursor_pos.get() > 0 && is_all_selected {
                text.modify(|text| {
                    let mut text = text.clone();
                    text.remove(*cursor_pos.get() - 1);
                    text
                });
                cursor_pos.set(cursor_pos - 1);
                selection_start.set(cursor_pos - 1);
                return;
            }

            if selection_start.get() > cursor_pos.get() {
                text.modify(|text| {
                    let mut text = text.clone();
                    text.replace_range(*cursor_pos.get()..(*selection_start.get()), "");
                    text
                });
                cursor_pos.set(*cursor_pos.get());
                selection_start.set(*cursor_pos.get());
            } else {
                text.modify(|text| {
                    let mut text = text.clone();
                    text.replace_range(*selection_start.get()..(*cursor_pos.get()), "");
                    text
                });
                cursor_pos.set(*selection_start.get());
            }
        }
        "Delete" => {
            if *cursor_pos.get() < text.len() {
                text.modify(|text| {
                    let mut text = text.clone();
                    text.remove(*cursor_pos.get());
                    text
                });
            }
        }
        "Left" => {
            if *cursor_pos.get() > 0 {
                if event.modifiers.shift && is_all_selected {
                    selection_start.set(*cursor_pos.get());
                }

                let new_cursor_pos = *cursor_pos.get() - 1;
                cursor_pos.set(new_cursor_pos);
                if !event.modifiers.shift {
                    selection_start.set(new_cursor_pos);
                }
            }
        }
        "Right" => {
            if *cursor_pos.get() < text.len() {
                if event.modifiers.shift && is_all_selected {
                    selection_start.set(*cursor_pos.get());
                }

                let new_cursor_pos = *cursor_pos.get() + 1;
                cursor_pos.set(new_cursor_pos);
                if !event.modifiers.shift {
                    selection_start.set(new_cursor_pos);
                }
            }
        }

        "Home" => {
            cursor_pos.set(0);
            if !event.modifiers.shift {
                selection_start.set(0);
            }
        }

        "End" => {
            cursor_pos.set(text.len());
            if !event.modifiers.shift {
                selection_start.set(text.len());
            }
        }

        "X" => {
            if event.modifiers.command && !is_all_selected {
                let mut ctx = ClipboardContext::new().unwrap();
                let (drained_text, start, _) = get_text_range();
                text.modify(|text| {
                    let mut text = text.clone();
                    text.replace_range(start..drained_text.len(), "");
                    text
                });
                ctx.set_contents(drained_text).unwrap();
                cursor_pos.set(start);
                selection_start.set(start);
            }
        }

        "A" => {
            println!("{:?}", event.modifiers);

            if event.modifiers.command {
                cursor_pos.set(text.len());
                selection_start.set(0);
            }
        }

        "C" => {
            if event.modifiers.command && !is_all_selected {
                let mut ctx = ClipboardContext::new().unwrap();
                let (drained_text, _, _) = get_text_range();
                ctx.set_contents(drained_text).unwrap();
            }
        }

        "V" => {
            if event.modifiers.command {
                let mut ctx = ClipboardContext::new().unwrap();
                let clipboard_text = ctx.get_contents().unwrap();
                text.modify(|text| {
                    let mut text = text.clone();

                    let (_, start, end) = get_text_range();
                    if start != end {
                        text.replace_range(start..end, &clipboard_text);
                        cursor_pos.set(start + clipboard_text.len());
                        selection_start.set(start + clipboard_text.len());
                        return text;
                    }
                    text.insert_str(*cursor_pos.get(), &clipboard_text);
                    text
                });
                cursor_pos.set(cursor_pos + clipboard_text.len());
                selection_start.set(cursor_pos + clipboard_text.len());
            }
        }

        _ => {}
    };

    let handle_input = move |event: Event<crate::vdom::events::Text>| {
        if event.char.is_control() {
            return;
        }

        if event.modifiers.command {
            return;
        }

        if *cursor_pos.get() != *selection_start.get() {
            let (drained_text, start, _) = get_text_range();
            text.modify(|text| {
                let mut text = text.clone();
                text.replace_range(start..start + drained_text.len(), "");
                text.insert(start, event.char);
                text
            });
            cursor_pos.set(start + 1);
            selection_start.set(start + 1);
        } else {
            let mut chars = text.chars().collect::<Vec<_>>();
            chars.insert(*cursor_pos.get(), event.char);
            text.set(chars.iter().collect());
            cursor_pos.set(cursor_pos + 1);
            selection_start.set(cursor_pos + 1);
        }
    };

    let selection_start = *selection_start.get() as i64;
    // println!("cursor: {}  start: {}", cursor_pos, selection_start);

    let cursor = if *is_focused.get() {
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
        cursor_visible: *cursor_visible.get(),
        selection_start: selection_start,

        "{text}"
      }
    }
}
