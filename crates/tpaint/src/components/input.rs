use std::rc::Rc;

use crate::{
    events::{ClickEvent, InputEvent},
    prelude::*,
};
use copypasta::{ClipboardContext, ClipboardProvider};
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

    // TODO: clean up
    let handle_input = move |event: Event<InputEvent>| {
        let mut text = text.make_mut();

        let range = *selection_start.get()..*cursor_pos.get();
        let is_selecting = range.start != range.end;

        // println!("is_selected {} range: {:?}", is_selecting, range);

        // use this for shortcuts
        let before_text = text.clone();
        match event.logical_key.clone() {
            winit::keyboard::Key::Character(c) => {
                match c.as_str() {
                    "c" => {
                        if is_selecting && event.state.state().command() {
                            let text = text[range].to_string();
                            let mut ctx = ClipboardContext::new().unwrap();
                            println!("copying: {:?}", text);
                            ctx.set_contents(text).unwrap();
                            return;
                        }
                    }
                    "x" => {
                        if is_selecting && event.state.state().command() {
                            let selected_text = text[range.clone()].to_string();
                            let mut ctx = ClipboardContext::new().unwrap();
                            ctx.set_contents(selected_text).unwrap();

                            text.replace_range(range.clone(), &"".to_string());
                            cursor_pos.set(range.start);
                            return;
                        }
                    }

                    "a" => {
                        if event.state.state().modifiers().state().control_key() {
                            return;
                        }
                    }

                    _ => {}
                }

                text.replace_range(range.clone(), &c.to_string());
                cursor_pos.set(range.start + 1);
                selection_start.set(range.start + 1);
            }
            winit::keyboard::Key::Named(named_key) => match named_key {
                winit::keyboard::NamedKey::Delete => {
                    if *cursor_pos.get() < text.len() {
                        if is_selecting {
                            text.replace_range(range.clone(), &"".to_string());
                            cursor_pos.set(range.start);
                            selection_start.set(range.start);
                        } else {
                            text.remove(*cursor_pos.get());
                            cursor_pos.set(*cursor_pos.get());
                            selection_start.set(*cursor_pos.get());
                        }
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
                        selection_start.set(*cursor_pos);
                    });

                    // if !event.state.state().shift() {}
                }
                winit::keyboard::NamedKey::ArrowRight => {
                    cursor_pos.with_mut(|cursor_pos| {
                        if *cursor_pos < text.len() {
                            *cursor_pos += 1;
                        }
                        selection_start.set(*cursor_pos);
                    });

                    // if !event.state.state().shift() {
                    // }
                }
                winit::keyboard::NamedKey::Backspace => {
                    if *cursor_pos.get() > 0 {
                        if is_selecting {
                            text.replace_range(range.clone(), &"".to_string());
                            cursor_pos.set(range.start);
                            selection_start.set(range.start);
                        } else {
                            text.remove(*cursor_pos.get() - 1);
                            cursor_pos.set(*cursor_pos.get() - 1);
                            selection_start.set(*cursor_pos.get() - 1);
                        }
                    }
                }
                winit::keyboard::NamedKey::Space => {
                    text.insert(*cursor_pos.get(), ' ');
                    cursor_pos.set(*cursor_pos.get() + 1);
                    selection_start.set(*cursor_pos.get() + 1);
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
        if let Some(f_cursor_pos) = event.text_cursor_position {
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

            let mut interval = tokio::time::interval(std::time::Duration::from_millis(500));
            interval.tick().await;

            loop {
                interval.tick().await;
                cursor_visible.set(!*cursor_visible.get());
            }
        },
    );

    render! {
      view {
        class: "focus:border-2 border-1 p-5 min-w-100 border-gray-300 flex-col text-black focus:border-black bg-white cursor-text {cx.props.class}",
        tabindex: 0,
        oninput: handle_input,
        onclick: handle_click,
        onfocus: move |_| {
            cursor_blinking.cancel(cx);
            cursor_blinking.restart();
            is_focused.set(true);
          },
        onblur: move |_| {
            cursor_blinking.cancel(cx);
            is_focused.set(false);
        },
        onselect: move |event| {
            selection_start.set(event.start_cursor.ccursor.index);
            cursor_pos.set(event.end_cursor.ccursor.index);
        },
        text_cursor: *cursor_pos.get() as i64,
        text_cursor_visible: *cursor_visible.get() && *is_focused.get(),

        "{text}"
      }
    }
}
