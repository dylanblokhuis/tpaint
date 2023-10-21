use epaint::{
    text::FontDefinitions,
    textures::{TextureOptions, TexturesDelta},
    vec2, ClippedPrimitive, ClippedShape, Color32, Fonts, Pos2, Rect, Shape, TessellationOptions,
    TextureId, TextureManager, Vec2, WHITE_UV,
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

        if vdom.dirty_nodes.is_empty() {
            return;
        }
        log::debug!("Nodes dirty {}", vdom.dirty_nodes.len());
        log::debug!("Calculating layout for {} nodes", vdom.nodes.len());

        // rect layout pass
        {
            let _guard =
                tracing::trace_span!("Renderer::calculate_layout rect layout pass").entered();

            vdom.nodes.get_mut(root_id).unwrap().attrs.insert(
                "class".into(),
                "w-full h-full overflow-y-scroll flex-nowrap items-start justify-start scrollbar-default".into(),
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

                    let node = vdom.nodes.get(*child).unwrap();
                    child_ids[i] = node.styling.node.unwrap();
                    count += 1;
                }

                taffy.set_children(parent_id, &child_ids[..count]).unwrap(); // Only pass the filled portion
                true
            });

            let _guard =
                tracing::trace_span!("Renderer::calculate_layout rect layout pass (taffy)")
                    .entered();
            taffy
                .compute_layout(
                    vdom.nodes.get(root_id).unwrap().styling.node.unwrap(),
                    available_space,
                )
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

            let _guard =
                tracing::trace_span!("Renderer::calculate_layout text layout (taffy)").entered();
            taffy
                .compute_layout(
                    vdom.nodes.get(root_id).unwrap().styling.node.unwrap(),
                    available_space,
                )
                .unwrap();
        }

        // generate natural content size for scrollable nodes
        {
            let _guard = tracing::trace_span!("Renderer::calculate_layout scroll pass").entered();

            let mut styles_to_reset = Vec::new();
            let root_taffy_id = vdom.nodes.get(root_id).unwrap().styling.node.unwrap();
            vdom.traverse_tree_mut(root_id, &mut |node| {
                let old_layout = *taffy.layout(node.styling.node.unwrap()).unwrap();
                let mut old_style = taffy.style(node.styling.node.unwrap()).unwrap().clone();
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

            let _guard =
                tracing::trace_span!("Renderer::calculate_layout scroll pass (taffy)").entered();

            taffy
                .compute_layout(root_taffy_id, available_space)
                .unwrap();
        }

        vdom.dirty_nodes.clear();
    }

    /// will compute the rects for all the nodes using the final computed layout
    #[tracing::instrument(skip_all, name = "Renderer::compute_rects")]
    pub fn compute_rects(&mut self, vdom: &mut VDom) {
        // Now we do a pass so we cache the computed layout in our VDom tree
        let root_id = vdom.get_root_id();
        vdom.traverse_tree_mut_with_parent_and_data(
            root_id,
            None,
            &Vec2::ZERO,
            &mut |node, parent, parent_location_offset| {
                let taffy_id = node.styling.node.unwrap();
                let layout = self.taffy.layout(taffy_id).unwrap();

                let parent_scroll_offset = parent
                    .map(|p| {
                        let scroll = p.scroll;
                        let parent_layout = self.taffy.layout(p.styling.node.unwrap()).unwrap();

                        Vec2::new(
                            scroll
                                .x
                                .min(p.natural_content_size.width - parent_layout.size.width)
                                .max(0.0),
                            scroll
                                .y
                                .min(p.natural_content_size.height - parent_layout.size.height)
                                .max(0.0),
                        )
                    })
                    .unwrap_or_default();

                let location = *parent_location_offset - parent_scroll_offset
                    + epaint::Vec2::new(layout.location.x, layout.location.y);

                node.computed_rect = epaint::Rect {
                    min: location.to_pos2(),
                    max: Pos2 {
                        x: location.x + layout.size.width,
                        y: location.y + layout.size.height,
                    },
                };

                (true, location)
            },
        );
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
            &None,
            &mut |node, parent, parent_clip: &Option<Rect>| {
                let taffy_id = node.styling.node.unwrap();
                let style = self.taffy.style(taffy_id).unwrap();

                // we need to make sure the scrollbar doesnt get overwritten
                let node_clip = {
                    epaint::Rect {
                        min: node.computed_rect.min,
                        max: epaint::Pos2 {
                            x: if style.overflow.y == Overflow::Scroll
                                && style.scrollbar_width != 0.0
                            {
                                node.computed_rect.max.x - style.scrollbar_width
                            } else {
                                node.computed_rect.max.x
                            },
                            y: if style.overflow.x == Overflow::Scroll
                                && style.scrollbar_width != 0.0
                            {
                                node.computed_rect.max.y - style.scrollbar_width
                            } else {
                                node.computed_rect.max.y
                            },
                        },
                    }
                };

                let mut clip = node_clip;
                match style.overflow.y {
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

                match &(*node.tag) {
                    "text" => {
                        let parent = parent.unwrap();
                        let shape = self.get_text_shape(node, parent, clip);

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
                                    shapes.extend_from_slice(&self.get_text_selection_shape(
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
                        shapes.push(self.get_rect_shape(node, clip));
                        let style = self.taffy.style(taffy_id).unwrap();

                        let are_both_scrollbars_visible = style.overflow.x == Overflow::Scroll
                            && style.overflow.y == Overflow::Scroll;

                        if style.scrollbar_width > 0.0 && style.overflow.y == Overflow::Scroll {
                            let (container, button) = self.get_scrollbar_shape(
                                node,
                                style.scrollbar_width,
                                false,
                                are_both_scrollbars_visible,
                                vdom.current_scroll_node
                                    .map(|scroll| {
                                        scroll.is_vertical_scrollbar_hovered && scroll.id == node.id
                                    })
                                    .unwrap_or(false),
                                vdom.current_scroll_node
                                    .map(|scroll| {
                                        (scroll.is_vertical_scrollbar_button_hovered
                                            || scroll.is_vertical_scrollbar_button_grabbed)
                                            && scroll.id == node.id
                                    })
                                    .unwrap_or(false),
                            );
                            shapes.push(container);
                            shapes.push(button);
                        }

                        if style.scrollbar_width > 0.0 && style.overflow.x == Overflow::Scroll {
                            let (container, button) = self.get_scrollbar_shape(
                                node,
                                style.scrollbar_width,
                                true,
                                are_both_scrollbars_visible,
                                vdom.current_scroll_node
                                    .map(|scroll| {
                                        scroll.is_horizontal_scrollbar_hovered
                                            && scroll.id == node.id
                                    })
                                    .unwrap_or(false),
                                vdom.current_scroll_node
                                    .map(|scroll| {
                                        (scroll.is_horizontal_scrollbar_hovered
                                            || scroll.is_horizontal_scrollbar_button_grabbed)
                                            && scroll.id == node.id
                                    })
                                    .unwrap_or(false),
                            );
                            shapes.push(container);
                            shapes.push(button);
                        }

                        if are_both_scrollbars_visible {
                            shapes.push(self.get_scrollbar_bottom_right_prop(
                                node,
                                &shapes[shapes.len() - 4],
                                &shapes[shapes.len() - 2],
                                style.scrollbar_width,
                            ))
                        }
                    }
                }

                (true, Some(clip))
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
        bar_width: f32,
        horizontal: bool,
        are_both_scrollbars_visible: bool,
    ) -> Rect {
        let styling = &node.styling;
        let location = node.computed_rect.min;
        let size = node.computed_rect.size();

        if horizontal {
            epaint::Rect {
                min: epaint::Pos2 {
                    x: location.x + styling.border.width / 2.0,
                    y: location.y + size.y - bar_width,
                },
                max: epaint::Pos2 {
                    x: location.x + size.x
                        - styling.border.width / 2.0
                        - if are_both_scrollbars_visible {
                            bar_width
                        } else {
                            0.0
                        },
                    y: location.y + size.y,
                },
            }
        } else {
            epaint::Rect {
                min: epaint::Pos2 {
                    x: location.x + size.x - bar_width,
                    y: location.y + styling.border.width / 2.0,
                },
                max: epaint::Pos2 {
                    x: location.x + size.x,
                    y: location.y + size.y
                        - styling.border.width / 2.0
                        - if are_both_scrollbars_visible {
                            bar_width
                        } else {
                            0.0
                        },
                },
            }
        }
    }

    pub fn get_scroll_thumb_rect(
        &self,
        node: &Node,
        bar_width: f32,
        horizontal: bool,
        are_both_scrollbars_visible: bool,
    ) -> Rect {
        let styling = &node.styling;
        let location = node.computed_rect.min;
        let size = node.computed_rect.size();

        let button_width = bar_width * 0.50; // 50% of bar_width

        if horizontal {
            let thumb_width = (size.x / node.natural_content_size.width)
                * (size.x
                    - styling.border.width
                    - if are_both_scrollbars_visible {
                        bar_width
                    } else {
                        0.0
                    });

            let thumb_max_x = size.x
                - styling.border.width
                - thumb_width
                - if are_both_scrollbars_visible {
                    bar_width
                } else {
                    0.0
                };
            let thumb_position_x =
                (node.scroll.x / (node.natural_content_size.width - size.x)) * thumb_max_x;

            epaint::Rect {
                min: epaint::Pos2 {
                    x: location.x + styling.border.width / 2.0 + thumb_position_x,
                    y: location.y + size.y - bar_width + (bar_width - button_width) / 2.0,
                },
                max: epaint::Pos2 {
                    x: location.x + styling.border.width / 2.0 + thumb_position_x + thumb_width,
                    y: location.y + size.y - bar_width + (bar_width + button_width) / 2.0,
                },
            }
        } else {
            let thumb_height = (size.y / node.natural_content_size.height)
                * (size.y
                    - styling.border.width
                    - if are_both_scrollbars_visible {
                        bar_width
                    } else {
                        0.0
                    });

            let thumb_max_y = size.y
                - styling.border.width
                - thumb_height
                - if are_both_scrollbars_visible {
                    bar_width
                } else {
                    0.0
                };
            let thumb_position_y =
                (node.scroll.y / (node.natural_content_size.height - size.y)) * thumb_max_y;

            epaint::Rect {
                min: epaint::Pos2 {
                    x: location.x + size.x - bar_width + (bar_width - button_width) / 2.0,
                    y: location.y + styling.border.width / 2.0 + thumb_position_y,
                },
                max: epaint::Pos2 {
                    x: location.x + size.x - bar_width + (bar_width + button_width) / 2.0,
                    y: location.y + styling.border.width / 2.0 + thumb_position_y + thumb_height,
                },
            }
        }
    }

    pub fn get_scrollbar_shape(
        &self,
        node: &Node,
        bar_width: f32,
        horizontal: bool,
        are_both_scrollbars_visible: bool,
        hovered: bool,
        thumb_hovered: bool,
    ) -> (ClippedShape, ClippedShape) {
        let styling = &node.styling;

        let container_shape = epaint::Shape::Rect(epaint::RectShape {
            rect: self.get_scrollbar_rect(node, bar_width, horizontal, are_both_scrollbars_visible),
            rounding: epaint::Rounding::ZERO,
            fill: if hovered {
                styling.scrollbar.background_color_hovered
            } else {
                styling.scrollbar.background_color
            },
            stroke: epaint::Stroke::NONE,
            fill_texture_id: TextureId::default(),
            uv: epaint::Rect::from_min_max(WHITE_UV, WHITE_UV),
        });

        let button_shape = epaint::Shape::Rect(epaint::RectShape {
            rect: self.get_scroll_thumb_rect(
                node,
                bar_width,
                horizontal,
                are_both_scrollbars_visible,
            ),
            rounding: epaint::Rounding {
                ne: 100.0,
                nw: 100.0,
                se: 100.0,
                sw: 100.0,
            },
            fill: if thumb_hovered {
                styling.scrollbar.thumb_color_hovered
            } else {
                styling.scrollbar.thumb_color
            },
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

    pub fn get_scrollbar_bottom_right_prop(
        &self,
        node: &Node,
        vertical_container: &ClippedShape,
        horizontal_container: &ClippedShape,
        bar_width: f32,
    ) -> ClippedShape {
        let vertical_container = vertical_container.shape.visual_bounding_rect();
        let horizontal_container = horizontal_container.shape.visual_bounding_rect();

        let shape = epaint::Shape::Rect(epaint::RectShape {
            rect: epaint::Rect {
                min: epaint::Pos2 {
                    x: horizontal_container.max.x,
                    y: vertical_container.max.y,
                },
                max: epaint::Pos2 {
                    x: horizontal_container.max.x + bar_width,
                    y: vertical_container.max.y + bar_width,
                },
            },
            rounding: epaint::Rounding::ZERO,
            fill: node.styling.scrollbar.background_color,
            stroke: epaint::Stroke::NONE,
            fill_texture_id: TextureId::default(),
            uv: epaint::Rect::from_min_max(WHITE_UV, WHITE_UV),
        });

        ClippedShape {
            clip_rect: shape.visual_bounding_rect(),
            shape,
        }
    }

    #[tracing::instrument(skip_all, name = "Renderer::get_text_shape")]
    fn get_text_shape(&self, node: &Node, parent_node: &Node, parent_clip: Rect) -> ClippedShape {
        let parent = &parent_node.styling;

        let galley = self.fonts.layout(
            node.attrs.get("value").unwrap().clone(),
            parent.text.font.clone(),
            parent.text.color,
            node.computed_rect.size().x + 1.0,
        );

        let shape = Shape::galley(node.computed_rect.min, galley);

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
    ) -> Vec<ClippedShape> {
        let cursor_rect = text_shape
            .galley
            .pos_from_pcursor(epaint::text::cursor::PCursor {
                paragraph: 0,
                offset: cursor_pos,
                prefer_next_row: false,
            });

        let selection_rect = text_shape
            .galley
            .pos_from_pcursor(epaint::text::cursor::PCursor {
                paragraph: 0,
                offset: selection_start,
                prefer_next_row: false,
            });

        let mut shapes = Vec::new();

        // swap if cursor is before selection

        let (start_cursor, end_cursor) = if cursor_pos < selection_start {
            let start_cursor = text_shape
                .galley
                .cursor_from_pos(cursor_rect.min.to_vec2() + cursor_rect.size());

            let end_cursor = text_shape
                .galley
                .cursor_from_pos(selection_rect.min.to_vec2() + selection_rect.size());

            (start_cursor, end_cursor)
        } else {
            let start_cursor = text_shape
                .galley
                .cursor_from_pos(selection_rect.min.to_vec2() + selection_rect.size());
            let end_cursor = text_shape
                .galley
                .cursor_from_pos(cursor_rect.min.to_vec2() + cursor_rect.size());

            (start_cursor, end_cursor)
        };

        let min = start_cursor.rcursor;
        let max = end_cursor.rcursor;

        for ri in min.row..=max.row {
            let row = &text_shape.galley.rows[ri];
            let left = if ri == min.row {
                row.x_offset(min.column)
            } else {
                row.rect.left()
            };
            let right = if ri == max.row {
                row.x_offset(max.column)
            } else {
                let newline_size = if row.ends_with_newline {
                    row.height() / 2.0 // visualize that we select the newline
                } else {
                    0.0
                };
                row.rect.right() + newline_size
            };
            let rect = Rect::from_min_max(
                text_shape.pos + vec2(left, row.min_y()),
                text_shape.pos + vec2(right, row.max_y()),
            );
            shapes.push(ClippedShape {
                clip_rect: rect,
                shape: epaint::Shape::Rect(epaint::RectShape {
                    rect,
                    rounding: epaint::Rounding::ZERO,
                    fill: selection_color,
                    stroke: epaint::Stroke::default(),
                    fill_texture_id: TextureId::default(),
                    uv: epaint::Rect::from_min_max(WHITE_UV, WHITE_UV),
                }),
            });
        }

        // let mut rect = if cursor_pos > selection_start {
        //     epaint::Rect::from_min_max(
        //         epaint::Pos2 {
        //             x: selection_rect.min.x,
        //             y: selection_rect.min.y,
        //         },
        //         epaint::Pos2 {
        //             x: cursor_rect.max.x,
        //             y: cursor_rect.max.y,
        //         },
        //     )
        // } else {
        //     epaint::Rect::from_min_max(
        //         epaint::Pos2 {
        //             x: cursor_rect.min.x,
        //             y: cursor_rect.min.y,
        //         },
        //         epaint::Pos2 {
        //             x: selection_rect.max.x,
        //             y: selection_rect.max.y,
        //         },
        //     )
        // };

        // rect.min.x += text_shape.pos.x;
        // rect.max.x += text_shape.pos.x;
        // rect.min.y += text_shape.pos.y;
        // rect.max.y += text_shape.pos.y;

        shapes
    }

    #[tracing::instrument(skip_all, name = "Renderer::get_rect_shape")]
    fn get_rect_shape(&self, node: &Node, parent_clip: Rect) -> ClippedShape {
        let styling = &node.styling;
        let rounding = styling.border.radius;
        let rect = epaint::Rect {
            min: epaint::Pos2 {
                x: node.computed_rect.min.x + styling.border.width / 2.0,
                y: node.computed_rect.min.y + styling.border.width / 2.0,
            },
            max: node.computed_rect.max,
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
