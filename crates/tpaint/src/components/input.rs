use std::rc::Rc;

use crate::{
    events::{ClickEvent, InputEvent},
    prelude::*,
};
use dioxus::prelude::*;

#[derive(Props)]
pub struct InputProps<'a> {
    #[props(default = "", into)]
    pub class: &'a str,
    pub onchange: Option<EventHandler<'a, Rc<String>>>,
    pub default_value: Option<&'a str>,
    pub value: Option<&'a str>,
}

pub fn Input<'a>(cx: Scope<'a, InputProps<'a>>) -> Element {
    let text = use_state(cx, || cx.props.default_value.unwrap_or("").to_string());
    let cursor_pos = use_state(cx, || 0);
    let cursor_visible = use_state(cx, || false);
    let is_focused = use_state(cx, || false);
    let selection_start = use_state(cx, || 0);

    // when this component is "controlled" by a value outside the scope, we need to update the text state
    let text = if let Some(value) = cx.props.value {
        let value = value.to_string();
        if value != *text.current() {
            text.set(value.clone());

            if *cursor_pos.get() > value.len() {
                cursor_pos.set(value.len());
            }
            if *selection_start.get() > value.len() {
                selection_start.set(value.len());
            }
        }
        text
    } else {
        text
    };

    let handle_input = move |event: Event<InputEvent>| {
        let mut text = text.make_mut();

        // println!("input: {:?}", event);
        // use this for shortcuts
        let before_text = text.clone();
        match event.logical_key.clone() {
            winit::keyboard::Key::Character(c) => {
                match c.as_str() {
                    "c" => {
                        // do stuff here
                    }
                    "x" => {
                        // do stuff here
                    }

                    _ => {}
                }

                text.insert_str(*cursor_pos.get(), &c.to_string());
                cursor_pos.with_mut(|cursor_pos| {
                    *cursor_pos += 1;
                });
            }
            winit::keyboard::Key::Named(named_key) => match named_key {
                winit::keyboard::NamedKey::Delete => {
                    if *cursor_pos.get() < text.len() {
                        text.remove(*cursor_pos.get());
                    }
                }
                winit::keyboard::NamedKey::Home => {
                    cursor_pos.set(0);
                }
                winit::keyboard::NamedKey::End => {
                    cursor_pos.set(text.len());
                }
                winit::keyboard::NamedKey::ArrowLeft => {
                    cursor_pos.with_mut(|cursor_pos| {
                        if *cursor_pos > 0 {
                            *cursor_pos -= 1;
                        }
                    });
                }
                winit::keyboard::NamedKey::ArrowRight => {
                    cursor_pos.with_mut(|cursor_pos| {
                        if *cursor_pos < text.len() {
                            *cursor_pos += 1;
                        }
                    });
                }
                winit::keyboard::NamedKey::Backspace => {
                    if *cursor_pos.get() > 0 {
                        text.remove(*cursor_pos.get() - 1);
                        cursor_pos.with_mut(|cursor_pos| {
                            *cursor_pos -= 1;
                        });
                    }
                }
                _ => {}
            },
            _ => {}
        }

        if before_text != *text {
            if let Some(onchange) = &cx.props.onchange {
                onchange.call(Rc::new(text.clone()));
            }
        }
    };

    let handle_click = move |event: Event<ClickEvent>| {
        let Some(focused) = event.state.focused else {
            return;
        };

        if let Some(f_cursor_pos) = focused.text_cursor {
            cursor_pos.set(f_cursor_pos);
        } else {
            cursor_pos.set(0);
            cursor_visible.set(false);
        }
    };

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

    render! {
      view {
        class: "focus:border-2 border-1 min-w-100 border-gray-300 flex-col text-black focus:border-black bg-white {cx.props.class}",
        tabindex: 0,
        oninput: handle_input,
        onclick: handle_click,
        text_cursor: *cursor_pos.get() as i64,

        onfocus: move |_| {
            cursor_blinking.cancel(cx);
            cursor_blinking.restart();
            is_focused.set(true);
          },
        onblur: move |_| {
            cursor_blinking.cancel(cx);
            is_focused.set(false);
        },
        text_cursor_visible: *cursor_visible.get() && *is_focused.get(),
        // onclick: move |_| {
        //     // text.set("bg-black".to_string());
        // },
        // onkeydown: handle_keydown,
        // oninput: handle_input,
        // onclick: handle_click,
        // ondrag: handle_drag,
        // text_cursor: *cursor_pos.get() as i64,
        // text_cursor_visible: *cursor_visible.get() && *is_focused.get(),
        // text_selection_start: *selection_start.get() as i64,
        // global_selection_mode: "off",

        "{text}"
      }
    }
}
