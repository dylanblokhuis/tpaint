# tpaint
Dioxus (GUI) + Taffy (Layout) + Tailwind (Styling) + epaint (Rendering)

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
  
### What needs implementing:
- Text selection and copying
- Links
- Clicking the scrollbars
- many more..


### Examples
Due to the nature of egui being ported to multiple backends already, thanks to all the egui contributors it was no effort to also add support for these in tpaint. 

- glow (OpenGL)
- wgpu

You can add your own backend easily!
