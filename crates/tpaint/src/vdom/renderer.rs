use epaint::{
    text::FontDefinitions,
    textures::{TextureOptions, TexturesDelta},
    ClippedPrimitive, ClippedShape, Color32, Fonts, TessellationOptions, TextureId, TextureManager,
    Vec2, WHITE_UV,
};

use taffy::{prelude::Size, Taffy};
use winit::dpi::PhysicalSize;

use super::{
    tailwind::{StyleState, Tailwind},
    Node, NodeId, ScreenDescriptor, VDom, MAX_CHILDREN,
};

pub const FOCUS_BORDER_WIDTH: f32 = 2.0;

pub struct Renderer {
    pub screen_descriptor: ScreenDescriptor,
    pub fonts: Fonts,
    pub tex_manager: TextureManager,
    pub taffy: Taffy,
}

impl Renderer {
    pub fn new(
        window_size: PhysicalSize<u32>,
        pixels_per_point: f32,
        definitions: FontDefinitions,
    ) -> Renderer {
        let fonts = Fonts::new(pixels_per_point, 1024, definitions);
        let mut tex_manager = TextureManager::default();
        let font_image_delta: Option<_> = fonts.font_image_delta();
        if let Some(font_image_delta) = font_image_delta {
            tex_manager.alloc(
                "fonts".into(),
                font_image_delta.image,
                TextureOptions::LINEAR,
            );
        }

        Renderer {
            screen_descriptor: ScreenDescriptor {
                pixels_per_point,
                size: window_size,
            },
            fonts,
            tex_manager,
            taffy: Taffy::new(),
        }
    }

    pub fn calculate_layout(&mut self, vdom: &mut VDom) {
        let root_id = vdom.get_root_id();

        // give root_node styling
        vdom.nodes
            .get_mut(root_id)
            .unwrap()
            .attrs
            .insert("class".into(), "w-full h-full".into());

        let taffy = &mut self.taffy;
        let hovered = vdom.hovered.clone();
        vdom.traverse_tree_mut_with_parent(root_id, None, &mut |node, parent| {
            match &(*node.tag) {
                "text" => {
                    let Some(text) = node.attrs.get("value") else {
                        return true;
                    };

                    let styling = node.styling.get_or_insert(Tailwind::default());
                    styling.set_text_styling(
                        text,
                        taffy,
                        &self.fonts,
                        parent.unwrap().styling.as_ref().unwrap(),
                    );
                }
                #[cfg(feature = "images")]
                "image" => {
                    let Some(src_attr) = node.attrs.get("src") else {
                        return true;
                    };

                    let styling = node.styling.get_or_insert(Tailwind::default());
                    styling
                        .set_styling(
                            taffy,
                            node.attrs.get("class").unwrap_or(&String::new()),
                            &StyleState {
                                hovered: hovered.contains(&node.id),
                                focused: false,
                            },
                        )
                        .set_texture(taffy, src_attr, &mut self.tex_manager);
                }
                _ => {
                    let Some(class_attr) = node.attrs.get("class") else {
                        return true;
                    };

                    let styling = node.styling.get_or_insert(Tailwind::default());
                    styling.set_styling(
                        taffy,
                        class_attr,
                        &StyleState {
                            hovered: hovered.contains(&node.id),
                            focused: false,
                        },
                    );
                }
            }

            true
        });

        // set all the newly created leaf nodes to their parents
        vdom.traverse_tree(root_id, &mut |node| {
            let Some(styling) = &node.styling else {
                return true;
            };
            let parent_id = styling.node.unwrap();
            let mut child_ids = [taffy::prelude::NodeId::new(0); MAX_CHILDREN];
            let mut count = 0; // Keep track of how many child_ids we've filled

            for (i, child) in node.children.iter().enumerate() {
                if i >= MAX_CHILDREN {
                    log::error!("Max children reached for node {:?}", node);
                    break;
                }
                if let Some(child_styling) = &vdom.nodes.get(*child).unwrap().styling {
                    child_ids[i] = child_styling.node.unwrap();
                    count += 1;
                }
            }

            taffy.set_children(parent_id, &child_ids[..count]).unwrap(); // Only pass the filled portion
            true
        });

        let node = vdom.nodes.get(root_id).unwrap();
        let styling = node.styling.as_ref().unwrap();
        taffy
            .compute_layout(
                styling.node.unwrap(),
                Size {
                    width: taffy::style::AvailableSpace::Definite(
                        self.screen_descriptor.size.width as f32,
                    ),
                    height: taffy::style::AvailableSpace::Definite(
                        self.screen_descriptor.size.height as f32,
                    ),
                },
            )
            .unwrap()
    }

    pub fn get_paint_info(
        &mut self,
        vdom: &VDom,
    ) -> (Vec<ClippedPrimitive>, TexturesDelta, &ScreenDescriptor) {
        let mut shapes = Vec::with_capacity(vdom.nodes.len());
        let root_id = vdom.get_root_id();

        vdom.traverse_tree_with_parent(root_id, None, &mut |node, parent| {
            let Some(styling) = &node.styling else {
                return true;
            };
            let taffy_id = styling.node.unwrap();
            let layout = self.taffy.layout(taffy_id).unwrap();
            let location = if let Some(parent) = parent {
                let parent_layout = self
                    .taffy
                    .layout(parent.styling.as_ref().unwrap().node.unwrap())
                    .unwrap();
                epaint::Vec2::new(parent_layout.location.x, parent_layout.location.y)
                    + epaint::Vec2::new(layout.location.x, layout.location.y)
            } else {
                epaint::Vec2::new(layout.location.x, layout.location.y)
            };

            match &(*node.tag) {
                "text" => {
                    shapes.push(self.get_text_shape(node, parent.unwrap(), layout, &location));
                }
                _ => {
                    shapes.push(self.get_rect_shape(node, vdom, styling, layout, &location));
                }
            }

            shapes.push(if node.tag == "text".into() {
                self.get_text_shape(node, parent.unwrap(), layout, &location)
            } else {
                self.get_rect_shape(node, vdom, styling, layout, &location)
            });
            true
        });

        let texture_delta = {
            let font_image_delta = self.fonts.font_image_delta();
            if let Some(font_image_delta) = font_image_delta {
                self.tex_manager.set(TextureId::default(), font_image_delta);
            }

            self.tex_manager.take_delta()
        };

        let (font_tex_size, prepared_discs) = {
            let atlas = self.fonts.texture_atlas();
            let atlas = atlas.lock();
            (atlas.size(), atlas.prepared_discs())
        };

        let primitives = {
            epaint::tessellator::tessellate_shapes(
                self.fonts.pixels_per_point(),
                TessellationOptions::default(),
                font_tex_size,
                prepared_discs,
                std::mem::take(&mut shapes),
            )
        };

        (primitives, texture_delta, &self.screen_descriptor)
    }

    fn get_text_shape(
        &self,
        node: &Node,
        parent_node: &Node,
        _layout: &taffy::prelude::Layout,
        location: &Vec2,
    ) -> ClippedShape {
        let parent = parent_node.styling.as_ref().unwrap();
        let _styling = node.styling.as_ref().unwrap();

        let shape = epaint::Shape::text(
            &self.fonts,
            epaint::Pos2 {
                x: location.x,
                y: location.y,
            },
            parent.text.align,
            node.attrs.get("value").unwrap(),
            parent.text.font.clone(),
            parent.text.color,
        );

        ClippedShape {
            clip_rect: shape.visual_bounding_rect(),
            shape,
        }
    }

    fn get_rect_shape(
        &self,
        node: &Node,
        vdom: &VDom,
        styling: &Tailwind,
        layout: &taffy::prelude::Layout,
        location: &Vec2,
    ) -> ClippedShape {
        let focused = if let Some(focused) = vdom.focused {
            focused
        } else {
            NodeId::default()
        };
        let border_width = if focused == node.id {
            FOCUS_BORDER_WIDTH
        } else {
            styling.border.width
        };
        let rounding = styling.border.radius;
        let x_start = location.x + border_width / 2.0;
        let y_start = location.y + border_width / 2.0;
        let x_end: f32 = location.x + layout.size.width - border_width / 2.0;
        let y_end: f32 = location.y + layout.size.height - border_width / 2.0;
        let rect = epaint::Rect {
            min: epaint::Pos2 {
                x: x_start,
                y: y_start,
            },
            max: epaint::Pos2 { x: x_end, y: y_end },
        };

        let shape = epaint::Shape::Rect(epaint::RectShape {
            rect,
            rounding,
            fill: if styling.texture_id.is_some() {
                Color32::WHITE
            } else {
                styling.background_color
            },
            stroke: epaint::Stroke {
                width: border_width,
                color: styling.border.color,
            },
            fill_texture_id: if let Some(texture_id) = styling.texture_id {
                texture_id
            } else {
                TextureId::default()
            },
            uv: if styling.texture_id.is_some() {
                epaint::Rect::from_min_max(epaint::pos2(0.0, 0.0), epaint::pos2(1.0, 1.0))
            } else {
                epaint::Rect::from_min_max(WHITE_UV, WHITE_UV)
            },
        });
        let clip = shape.visual_bounding_rect();

        ClippedShape {
            clip_rect: clip,
            shape,
        }
    }
}
