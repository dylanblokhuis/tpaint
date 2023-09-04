# tpaint
Dioxus (GUI) + Taffy (Layout) + Tailwind (Styling) + epaint (Rendering)

### Currently supports:

- Images through img tag, with 'src' attribute. Disk only and supports svg, png and jpg. More can easily be added through the ``image`` crate
- Background color
- Border
- Border Radius
- Text
- Text color
- Flexbox
- Grid
- Hot reloading, use the ``hot-reload`` feature

### What needs implementing:

- Fixing the focus handler, currently disabled since it crashed whenever I ported this from Blitz
- Better Mouse position to UI node performance? Blitz uses a quadtree but was slow on updating, tpaint's current implementation is a naive recursive loop
- Loading images from network instead of only from disk
- Text wrapping
- Input field and their type implementation
- Select field
- Textarea field
- Forms
- many more..


### Examples
Due to the nature of egui being ported to multiple backends already, thanks to all the egui contributors it was no effort to also add support for these in tpaint. 

- glow (OpenGL)
- wgpu
