# tpaint
Dioxus (GUI) + Taffy (Layout) + Tailwind (Styling) + epaint (Rendering)

### Currently supports:

- Background color
- Border
- Border Radius
- Text
- Text color
- Flexbox
- Grid
- Hot reloading, use the ``hot-reload`` feature
- Scrolling
- Images through img tag, with 'src' attribute. Disk only and supports svg, png and jpg. More can easily be added through the ``image`` crate
- Input text field
  
### What needs implementing:

- Lazy layout
- Lazy rendering (only whats visible)
- Loading images from network instead of only from disk
- Number field with drag value
- Checkbox field
- Radio field
- Select field
- Textarea field
- Links
- many more..


### Examples
Due to the nature of egui being ported to multiple backends already, thanks to all the egui contributors it was no effort to also add support for these in tpaint. 

- glow (OpenGL)
- wgpu
