use epaint::{
    text::FontDefinitions,
    textures::{TextureOptions, TexturesDelta},
    ClippedPrimitive, ClippedShape, Color32, Fonts, Rect, Shape, TessellationOptions, TextureId,
    TextureManager, Vec2, WHITE_UV,
};

use taffy::{prelude::Size, Taffy};
use winit::dpi::PhysicalSize;

use super::{tailwind::StyleState, Node, ScreenDescriptor, VDom, MAX_CHILDREN};

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
        let focused = vdom.focused;
        vdom.traverse_tree_mut_with_parent(root_id, None, &mut |node, parent| {
            let style_state = StyleState {
                hovered: hovered.contains(&node.id),
                focused: focused.map(|id| id == node.id).unwrap_or(false),
            };

            match &(*node.tag) {
                "text" => {
                    node.styling.set_text_styling(
                        node.attrs.get("value").unwrap_or(&String::new()),
                        taffy,
                        &self.fonts,
                        &parent.unwrap().styling,
                    );
                }
                #[cfg(feature = "images")]
                "image" => {
                    node.styling
                        .set_styling(
                            taffy,
                            node.attrs.get("class").unwrap_or(&String::new()),
                            &style_state,
                        )
                        .set_texture(
                            taffy,
                            node.attrs.get("src").unwrap_or(&String::new()),
                            &mut self.tex_manager,
                        );
                }
                _ => {
                    node.styling.set_styling(
                        taffy,
                        node.attrs.get("class").unwrap_or(&String::new()),
                        &style_state,
                    );
                }
            }

            true
        });

        // set all the newly created leaf nodes to their parents
        vdom.traverse_tree(root_id, &mut |node| {
            let parent_id = node.styling.node.unwrap();
            let mut child_ids = [taffy::prelude::NodeId::new(0); MAX_CHILDREN];
            let mut count = 0; // Keep track of how many child_ids we've filled

            for (i, child) in node.children.iter().enumerate() {
                if i >= MAX_CHILDREN {
                    log::error!("Max children reached for node {:?}", node);
                    break;
                }

                let node = vdom.nodes.get(*child).unwrap();
                child_ids[i] = node.styling.node.unwrap();
                count += 1;
            }

            taffy.set_children(parent_id, &child_ids[..count]).unwrap(); // Only pass the filled portion
            true
        });

        let node = vdom.nodes.get(root_id).unwrap();
        taffy
            .compute_layout(
                node.styling.node.unwrap(),
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

        vdom.traverse_tree_with_parent_and_data(
            root_id,
            None,
            &Vec2::ZERO,
            &mut |node, parent, parent_location_offset| {
                let taffy_id = node.styling.node.unwrap();
                let layout = self.taffy.layout(taffy_id).unwrap();
                let location = *parent_location_offset
                    + epaint::Vec2::new(layout.location.x, layout.location.y);

                match &(*node.tag) {
                    "text" => {
                        let parent = parent.unwrap();
                        let shape = self.get_text_shape(node, parent, layout, &location);

                        if let Some(cursor) = parent.attrs.get("cursor") {
                            let epaint::Shape::Text(text_shape) = &shape.shape else {
                                unreachable!();
                            };

                            if let Ok(cursor) = str::parse::<usize>(cursor) {
                                shapes.push(self.get_cursor_shape(text_shape, cursor));
                            }
                        }

                        shapes.push(shape);
                    }
                    _ => {
                        shapes.push(self.get_rect_shape(node, layout, &location));
                    }
                }

                (true, location)
            },
        );

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
        layout: &taffy::prelude::Layout,
        location: &Vec2,
    ) -> ClippedShape {
        let parent = &parent_node.styling;

        let galley = self.fonts.layout_no_wrap(
            node.attrs.get("value").unwrap().clone(),
            parent.text.font.clone(),
            parent.text.color,
        );

        let rect: Rect = Rect::from_min_size(
            epaint::Pos2 {
                x: location.x,
                y: location.y,
            },
            galley.size(),
        );

        let shape = Shape::galley(rect.min, galley);

        ClippedShape {
            clip_rect: shape.visual_bounding_rect(),
            shape,
        }
    }

    fn get_cursor_shape(&self, text_shape: &epaint::TextShape, cursor_pos: usize) -> ClippedShape {
        let rect = text_shape
            .galley
            .pos_from_cursor(&epaint::text::cursor::Cursor {
                pcursor: epaint::text::cursor::PCursor {
                    paragraph: 0,
                    offset: cursor_pos,
                    prefer_next_row: false,
                },
                ..Default::default()
            });

        let mut rect = rect;

        rect.min.x += text_shape.pos.x;
        rect.max.x += text_shape.pos.x;
        rect.min.y += text_shape.pos.y;
        rect.max.y += text_shape.pos.y;

        rect.min.x -= 0.5;
        rect.max.x += 0.5;

        ClippedShape {
            clip_rect: rect,
            shape: epaint::Shape::Rect(epaint::RectShape {
                rect,
                rounding: epaint::Rounding::ZERO,
                fill: Color32::BLACK,
                stroke: epaint::Stroke::default(),
                fill_texture_id: TextureId::default(),
                uv: epaint::Rect::from_min_max(WHITE_UV, WHITE_UV),
            }),
        }
    }

    fn get_rect_shape(
        &self,
        node: &Node,
        layout: &taffy::prelude::Layout,
        location: &Vec2,
    ) -> ClippedShape {
        let styling = &node.styling;
        let rounding = styling.border.radius;
        let x_start = location.x + styling.border.width / 2.0;
        let y_start = location.y + styling.border.width / 2.0;
        let x_end: f32 = location.x + layout.size.width - styling.border.width / 2.0;
        let y_end: f32 = location.y + layout.size.height - styling.border.width / 2.0;
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
                width: styling.border.width,
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

    // fn get_cursor_shape(
    //     &self,

    // )

    // pub fn print_taffy_tree(&self, taffy_root: taffy::prelude::NodeId, depth: usize) {
    //     let root_node = self.taffy.layout(taffy_root).unwrap();

    //     println!("{}{:?}", " ".repeat(depth), root_node);

    //     dbg!(self.taf)
    // }
}
