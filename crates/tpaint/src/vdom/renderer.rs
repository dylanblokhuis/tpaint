use epaint::{
    text::FontDefinitions,
    textures::{TextureOptions, TexturesDelta},
    ClippedPrimitive, ClippedShape, Color32, Fonts, Rect, Shape, TessellationOptions, TextureId,
    TextureManager, Vec2, WHITE_UV,
};

use taffy::{
    prelude::Size,
    style::{Dimension, Overflow},
    style_helpers::TaffyAuto,
    Taffy,
};
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

    #[tracing::instrument(skip_all, name = "Renderer::calculate_layout")]
    pub fn calculate_layout(&mut self, vdom: &mut VDom) {
        let root_id = vdom.get_root_id();
        let taffy = &mut self.taffy;
        let available_space = Size {
            width: taffy::style::AvailableSpace::Definite(
                self.screen_descriptor.size.width as f32 / self.screen_descriptor.pixels_per_point,
            ),
            height: taffy::style::AvailableSpace::Definite(
                self.screen_descriptor.size.height as f32 / self.screen_descriptor.pixels_per_point,
            ),
        };

        // rect layout pass
        {
            let _guard =
                tracing::trace_span!("Renderer::calculate_layout rect layout pass").entered();

            vdom.nodes[root_id].attrs.insert(
                "class".into(),
                "w-full h-full flex-nowrap flex-col items-start justify-start overflow-y-scroll scrollbar-default".into(),
            );

            let hovered = vdom.hovered.clone();
            let focused = vdom.focused;
            vdom.traverse_tree_mut(root_id, &mut |node| {
                let style_state = StyleState {
                    hovered: hovered.contains(&node.id),
                    focused: focused.map(|id| id == node.id).unwrap_or(false),
                };

                match &(*node.tag) {
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
                    "view" => {
                        node.styling.set_styling(
                            taffy,
                            node.attrs.get("class").unwrap_or(&String::new()),
                            &style_state,
                        );
                    }

                    "text" => {
                        node.styling.set_styling(taffy, "w-full", &style_state);
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

                    child_ids[i] = vdom.nodes[*child].styling.node.unwrap();
                    count += 1;
                }

                taffy.set_children(parent_id, &child_ids[..count]).unwrap(); // Only pass the filled portion
                true
            });

            taffy
                .compute_layout(vdom.nodes[root_id].styling.node.unwrap(), available_space)
                .unwrap();
        }

        // text pass, todo: DRY this
        {
            let _guard =
                tracing::trace_span!("Renderer::calculate_layout text layout pass").entered();

            vdom.traverse_tree_mut_with_parent(root_id, None, &mut |node, parent| {
                match &*node.tag {
                    "text" => {
                        node.styling.set_text_styling(
                            node.attrs.get("value").unwrap_or(&String::new()),
                            taffy,
                            &self.fonts,
                            &parent.unwrap().styling,
                        );
                    }
                    _ => {}
                }

                return true;
            });

            taffy
                .compute_layout(vdom.nodes[root_id].styling.node.unwrap(), available_space)
                .unwrap();
        }

        // generate natural content size for scrollable nodes
        {
            let _guard = tracing::trace_span!("Renderer::calculate_layout scroll pass").entered();

            let mut styles_to_reset = Vec::new();
            let root_taffy_id = vdom.nodes[root_id].styling.node.unwrap();
            vdom.traverse_tree_mut(root_id, &mut |node| {
                let mut old_style = taffy.style(node.styling.node.unwrap()).unwrap().clone();
                let old_layout = *taffy.layout(node.styling.node.unwrap()).unwrap();
                if old_style.overflow.y != Overflow::Scroll
                    && old_style.overflow.x != Overflow::Scroll
                {
                    return true;
                }

                let mut style = old_style.clone();
                style.overflow.x = Overflow::Visible;
                style.overflow.y = Overflow::Visible;
                style.size.width = Dimension::AUTO;
                style.size.height = Dimension::AUTO;

                taffy.set_style(node.styling.node.unwrap(), style).unwrap();
                taffy
                    .compute_layout(root_taffy_id, available_space)
                    .unwrap();

                let natural_layout = taffy.layout(node.styling.node.unwrap()).unwrap();
                node.natural_content_size = Size {
                    width: natural_layout.size.width,
                    height: natural_layout.size.height,
                };

                if old_layout.size.height > natural_layout.size.height
                    && old_layout.size.width > natural_layout.size.width
                {
                    old_style.scrollbar_width = 0.0;
                }

                styles_to_reset.push((node.styling.node.unwrap(), old_style));

                true
            });

            for (taffy_id, style) in styles_to_reset {
                taffy.set_style(taffy_id, style).unwrap();
            }

            taffy
                .compute_layout(root_taffy_id, available_space)
                .unwrap();
        }
    }

    #[tracing::instrument(skip_all, name = "Renderer::get_paint_info")]
    pub fn get_paint_info(
        &mut self,
        vdom: &VDom,
    ) -> (Vec<ClippedPrimitive>, TexturesDelta, &ScreenDescriptor) {
        let mut shapes = Vec::with_capacity(vdom.nodes.len());
        let root_id = vdom.get_root_id();
        vdom.traverse_tree_with_parent_and_data(
            root_id,
            None,
            &(Vec2::ZERO, None),
            &mut |node, parent, (parent_location_offset, parent_clip): &(Vec2, Option<Rect>)| {
                let taffy_id = node.styling.node.unwrap();
                let layout = self.taffy.layout(taffy_id).unwrap();

                let max_scroll = parent.map(|p| p.natural_content_size).unwrap_or_default();

                let parent_scroll_offset = parent
                    .map(|p| {
                        let scroll = p.scroll;
                        let parent_layout = self.taffy.layout(p.styling.node.unwrap()).unwrap();

                        Vec2::new(
                            scroll
                                .x
                                .min(max_scroll.width - parent_layout.size.width)
                                .max(0.0),
                            scroll
                                .y
                                .min(max_scroll.height - parent_layout.size.height)
                                .max(0.0),
                        )
                    })
                    .unwrap_or_default();

                let location = *parent_location_offset - parent_scroll_offset
                    + epaint::Vec2::new(layout.location.x, layout.location.y);

                let node_clip = {
                    epaint::Rect {
                        min: epaint::Pos2 {
                            x: location.x,
                            y: location.y,
                        },
                        max: epaint::Pos2 {
                            x: location.x + layout.size.width,
                            y: location.y + layout.size.height,
                        },
                    }
                };

                let style = self.taffy.style(taffy_id).unwrap();

                let mut clip = node_clip;
                let mut is_any_clipped = false;
                match style.overflow.y {
                    Overflow::Scroll | Overflow::Hidden => {
                        if let Some(current_clip) = parent_clip {
                            clip = node_clip.intersect(*current_clip);
                            is_any_clipped = true;
                        }
                    }
                    Overflow::Visible => {
                        if let Some(parent_clip_rect) = parent_clip {
                            clip = *parent_clip_rect;
                        }
                    }
                }

                if !is_any_clipped {
                    match style.overflow.x {
                        Overflow::Scroll | Overflow::Hidden => {
                            if let Some(current_clip) = parent_clip {
                                clip = node_clip.intersect(*current_clip);
                            }
                        }
                        Overflow::Visible => {
                            if let Some(parent_clip_rect) = parent_clip {
                                clip = *parent_clip_rect;
                            }
                        }
                    }
                }

                match &(*node.tag) {
                    "text" => {
                        let parent = parent.unwrap();
                        let shape = self.get_text_shape(node, parent, clip, layout, &location);

                        if let Some(cursor) = parent.attrs.get("cursor") {
                            let epaint::Shape::Text(text_shape) = &shape.shape else {
                                unreachable!();
                            };
                            let Some(selection_start) =
                                parent.attrs.get("selection_start").or(Some(cursor))
                            else {
                                unreachable!();
                            };

                            if let Ok(cursor) = str::parse::<usize>(cursor) {
                                if parent.attrs.get("cursor_visible").unwrap_or(&String::new())
                                    == "true"
                                {
                                    shapes.push(self.get_cursor_shape(text_shape, cursor));
                                }

                                if let Ok(selection_start) = str::parse::<usize>(selection_start) {
                                    shapes.push(self.get_text_selection_shape(
                                        text_shape,
                                        cursor,
                                        selection_start,
                                        parent.styling.text.selection_color,
                                    ));
                                }
                            }
                        }

                        shapes.push(shape);
                    }
                    _ => {
                        shapes.push(self.get_rect_shape(node, clip, layout, &location));

                        let style = self.taffy.style(taffy_id).unwrap();
                        if style.scrollbar_width > 0.0 && style.overflow.x == Overflow::Scroll {
                            let (container, button) = self.get_scrollbar_shape(
                                node,
                                style.scrollbar_width,
                                layout,
                                &location,
                                true,
                            );
                            shapes.push(container);
                            shapes.push(button);
                        }

                        if style.scrollbar_width > 0.0 && style.overflow.y == Overflow::Scroll {
                            let (container, button) = self.get_scrollbar_shape(
                                node,
                                style.scrollbar_width,
                                layout,
                                &location,
                                false,
                            );
                            shapes.push(container);
                            shapes.push(button);
                        }
                    }
                }

                (true, (location, Some(clip)))
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

        let primitives = epaint::tessellator::tessellate_shapes(
            self.fonts.pixels_per_point(),
            TessellationOptions::default(),
            font_tex_size,
            prepared_discs,
            std::mem::take(&mut shapes),
        );

        (primitives, texture_delta, &self.screen_descriptor)
    }

    pub fn get_scrollbar_rect(
        &self,
        node: &Node,
        layout: &taffy::prelude::Layout,
        location: &Vec2,
        bar_width: f32,
        horizontal: bool,
    ) -> Rect {
        let styling = &node.styling;

        if horizontal {
            epaint::Rect {
                min: epaint::Pos2 {
                    x: location.x + styling.border.width / 2.0,
                    y: location.y + layout.size.height - bar_width,
                },
                max: epaint::Pos2 {
                    x: location.x + layout.size.width - styling.border.width / 2.0,
                    y: location.y + layout.size.height,
                },
            }
        } else {
            epaint::Rect {
                min: epaint::Pos2 {
                    x: location.x + layout.size.width - bar_width,
                    y: location.y + styling.border.width / 2.0,
                },
                max: epaint::Pos2 {
                    x: location.x + layout.size.width,
                    y: location.y + layout.size.height - styling.border.width / 2.0,
                },
            }
        }
    }

    pub fn get_scroll_thumb_rect(
        &self,
        node: &Node,
        layout: &taffy::prelude::Layout,
        location: &Vec2,
        bar_width: f32,
        horizontal: bool,
    ) -> Rect {
        let styling = &node.styling;

        let button_width = bar_width * 0.50; // 50% of bar_width

        if horizontal {
            let thumb_width = (layout.size.width / node.natural_content_size.width)
                * (layout.size.width - styling.border.width);
            let thumb_position_x = (node.scroll.x
                / (node.natural_content_size.width - layout.size.width))
                * (layout.size.width - styling.border.width - thumb_width);

            epaint::Rect {
                min: epaint::Pos2 {
                    x: location.x + styling.border.width / 2.0 + thumb_position_x,
                    y: location.y + layout.size.height - bar_width
                        + (bar_width - button_width) / 2.0,
                },
                max: epaint::Pos2 {
                    x: location.x + styling.border.width / 2.0 + thumb_position_x + thumb_width,
                    y: location.y + layout.size.height,
                },
            }
        } else {
            let thumb_height = (layout.size.height / node.natural_content_size.height)
                * (layout.size.height - styling.border.width);

            let thumb_position_y = (node.scroll.y
                / (node.natural_content_size.height - layout.size.height))
                * (layout.size.height - styling.border.width - thumb_height);

            epaint::Rect {
                min: epaint::Pos2 {
                    x: location.x + layout.size.width - bar_width
                        + (bar_width - button_width) / 2.0,
                    y: location.y + styling.border.width / 2.0 + thumb_position_y,
                },
                max: epaint::Pos2 {
                    x: location.x + layout.size.width - bar_width
                        + (bar_width + button_width) / 2.0,
                    y: location.y + styling.border.width / 2.0 + thumb_position_y + thumb_height,
                },
            }
        }
    }

    pub fn get_scrollbar_shape(
        &self,
        node: &Node,
        bar_width: f32,
        layout: &taffy::prelude::Layout,
        location: &Vec2,
        horizontal: bool,
    ) -> (ClippedShape, ClippedShape) {
        let container_shape = epaint::Shape::Rect(epaint::RectShape {
            rect: self.get_scrollbar_rect(node, layout, location, bar_width, horizontal),
            rounding: epaint::Rounding::ZERO,
            fill: Color32::BLACK,
            stroke: epaint::Stroke::NONE,
            fill_texture_id: TextureId::default(),
            uv: epaint::Rect::from_min_max(WHITE_UV, WHITE_UV),
        });

        let button_shape = epaint::Shape::Rect(epaint::RectShape {
            rect: self.get_scroll_thumb_rect(node, layout, location, bar_width, horizontal),
            rounding: epaint::Rounding {
                ne: 100.0,
                nw: 100.0,
                se: 100.0,
                sw: 100.0,
            },
            fill: Color32::GRAY,
            stroke: epaint::Stroke::NONE,
            fill_texture_id: TextureId::default(),
            uv: epaint::Rect::from_min_max(WHITE_UV, WHITE_UV),
        });

        (
            ClippedShape {
                clip_rect: container_shape.visual_bounding_rect(),
                shape: container_shape,
            },
            ClippedShape {
                clip_rect: button_shape.visual_bounding_rect(),
                shape: button_shape,
            },
        )
    }

    #[tracing::instrument(skip_all, name = "Renderer::get_text_shape")]
    fn get_text_shape(
        &self,
        node: &Node,
        parent_node: &Node,
        parent_clip: Rect,
        layout: &taffy::prelude::Layout,
        location: &Vec2,
    ) -> ClippedShape {
        let parent = &parent_node.styling;

        let galley = self.fonts.layout(
            node.attrs.get("value").unwrap().clone(),
            parent.text.font.clone(),
            parent.text.color,
            layout.size.width + 0.5,
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
            clip_rect: parent_clip,
            shape,
        }
    }

    #[tracing::instrument(skip_all, name = "Renderer::get_cursor_shape")]
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

    #[tracing::instrument(skip_all, name = "Renderer::get_cursor_shape")]
    fn get_text_selection_shape(
        &self,
        text_shape: &epaint::TextShape,
        cursor_pos: usize,
        selection_start: usize,
        selection_color: Color32,
    ) -> ClippedShape {
        let cursor_rect = text_shape
            .galley
            .pos_from_cursor(&epaint::text::cursor::Cursor {
                pcursor: epaint::text::cursor::PCursor {
                    paragraph: 0,
                    offset: cursor_pos,
                    prefer_next_row: false,
                },
                ..Default::default()
            });

        let selection_rect = text_shape
            .galley
            .pos_from_cursor(&epaint::text::cursor::Cursor {
                pcursor: epaint::text::cursor::PCursor {
                    paragraph: 0,
                    offset: selection_start,
                    prefer_next_row: false,
                },
                ..Default::default()
            });

        let mut rect = if cursor_pos > selection_start {
            epaint::Rect::from_min_max(
                epaint::Pos2 {
                    x: selection_rect.min.x,
                    y: selection_rect.min.y,
                },
                epaint::Pos2 {
                    x: cursor_rect.max.x,
                    y: cursor_rect.max.y,
                },
            )
        } else {
            epaint::Rect::from_min_max(
                epaint::Pos2 {
                    x: cursor_rect.min.x,
                    y: cursor_rect.min.y,
                },
                epaint::Pos2 {
                    x: selection_rect.max.x,
                    y: selection_rect.max.y,
                },
            )
        };

        rect.min.x += text_shape.pos.x;
        rect.max.x += text_shape.pos.x;
        rect.min.y += text_shape.pos.y;
        rect.max.y += text_shape.pos.y;

        ClippedShape {
            clip_rect: rect,
            shape: epaint::Shape::Rect(epaint::RectShape {
                rect,
                rounding: epaint::Rounding::ZERO,
                fill: selection_color,
                stroke: epaint::Stroke::default(),
                fill_texture_id: TextureId::default(),
                uv: epaint::Rect::from_min_max(WHITE_UV, WHITE_UV),
            }),
        }
    }

    #[tracing::instrument(skip_all, name = "Renderer::get_rect_shape")]
    fn get_rect_shape(
        &self,
        node: &Node,
        parent_clip: Rect,
        layout: &taffy::prelude::Layout,
        location: &Vec2,
    ) -> ClippedShape {
        let styling = &node.styling;
        let rounding = styling.border.radius;
        let rect = epaint::Rect {
            min: epaint::Pos2 {
                x: location.x + styling.border.width / 2.0,
                y: location.y + styling.border.width / 2.0,
            },
            max: epaint::Pos2 {
                x: location.x + layout.size.width,
                y: location.y + layout.size.height,
            },
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

        ClippedShape {
            clip_rect: parent_clip,
            shape,
        }
    }
}
