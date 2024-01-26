# tpaint
Compose your UI with Dioxus (React-inspired) and Tailwind ergonomics and bring your own rendering backend to paint the triangles to the screen.

### Currently supports:

- Background color
- Border
- Border Radius
- Text
- Text color
- Hot reloading, use the ``hot-reload`` feature
- Scrolling
- Async images and vector graphics through ``Image`` component, with ``src`` attribute.
- Grid and flexbox (through Taffy)
- Text selection
- Cursors with e.g. ``cursor-progress``
- Input field
- Custom fonts

### Examples
tpaint uses egui's rasterization backend, so adding your backend is trivial!

Current examples include:

- glow (OpenGL)
- wgpu


### Element
``view`` is the only element you can compose your UI's with, it supports various events:

- onfocus
- onblur
- ondrag
- oninput
- onkeydown
- onkeyup
- onclick
- onmousemove
- onlayout (``whenever the layout engine has re-calculated the layout``)
- onselect


```rust
view {
    class: "h-40 p-10 bg-red-900 text-white",
    onclick: move |_| {
        println!("Clicked");
    },
    "I am a button"
}
```
