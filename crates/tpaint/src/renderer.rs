use std::time::Instant;

use epaint::{
    text::FontDefinitions,
    textures::{TextureOptions, TexturesDelta},
    vec2, ClippedPrimitive, ClippedShape, Color32, Fonts, Pos2, Primitive, Rect, Shape,
    TessellationOptions, Tessellator, TextureId, TextureManager, Vec2, WHITE_UV,
};

use taffy::{AvailableSpace, Layout, NodeId, Overflow, Size};
use winit::dpi::PhysicalSize;

use crate::{
    dom::{CursorState, Dom, NodeContext, SelectedNode},
    tailwind::{StyleState, TailwindCache},
};

#[derive(Clone, Debug)]
pub struct ScreenDescriptor {
    pub pixels_per_point: f32,
    pub size: PhysicalSize<u32>,
}
pub struct Renderer {
    pub screen_descriptor: ScreenDescriptor,
    pub fonts: Fonts,
    pub tex_manager: TextureManager,
    pub shapes: Vec<ClippedShape>,
    pub tessellator: Tessellator,
}

impl Renderer {
    pub fn new(
        window_size: PhysicalSize<u32>,
        pixels_per_point: f32,
        definitions: FontDefinitions,
    ) -> Renderer {
        let fonts = Fonts::new(pixels_per_point, 4096, definitions);
        let mut tex_manager = TextureManager::default();
        let font_image_delta: Option<_> = fonts.font_image_delta();
        if let Some(font_image_delta) = font_image_delta {
            tex_manager.alloc(
                "fonts".into(),
                font_image_delta.image,
                TextureOptions::LINEAR,
            );
        }

        let (font_tex_size, prepared_discs) = {
            let atlas = fonts.texture_atlas();
            let atlas = atlas.lock();
            (atlas.size(), atlas.prepared_discs())
        };

        let tessellator = Tessellator::new(
            fonts.pixels_per_point(),
            TessellationOptions::default(),
            font_tex_size,
            prepared_discs,
        );

        Renderer {
            screen_descriptor: ScreenDescriptor {
                pixels_per_point,
                size: window_size,
            },
            fonts,
            tex_manager,
            shapes: Vec::new(),
            tessellator,
        }
    }

    #[tracing::instrument(skip_all, name = "Renderer::calculate_layout")]
    pub fn calculate_layout(&mut self, dom: &mut Dom) {
        let root_id = dom.get_root_id();
        let available_space = Size {
            width: taffy::style::AvailableSpace::Definite(
                (self.screen_descriptor.size.width as f32
                    / self.screen_descriptor.pixels_per_point)
                    .ceil(),
            ),
            height: taffy::style::AvailableSpace::Definite(
                (self.screen_descriptor.size.height as f32
                    / self.screen_descriptor.pixels_per_point)
                    .ceil(),
            ),
        };

        // rect layout pass
        {
            let _guard =
                tracing::trace_span!("Renderer::calculate_layout rect layout pass").entered();

            dom.tree
                .get_node_context_mut(root_id)
                .unwrap()
                .attrs
                .insert("class".into(), "w-full h-full".into());

            dom.traverse_tree_with_parent(root_id, None, &mut |dom, id, parent| {
                let node = dom.tree.get_node_context_mut(id).unwrap();

                let style_state = StyleState {
                    hovered: dom.state.hovered.contains(&id),
                    focused: dom
                        .state
                        .focused
                        .as_ref()
                        .map(|id2| id2.node_id == id)
                        .unwrap_or(false),
                };

                let class = node.attrs.get("class");
                let styling_hash = TailwindCache {
                    class: class.cloned(),
                    state: style_state.clone(),
                };

                if node.styling.cache == styling_hash {
                    return true;
                }
                node.styling.cache = styling_hash;

                let style = match &(*node.tag) {
                    #[cfg(feature = "images")]
                    "image" => {
                        let mut style = node
                            .styling
                            .set_styling(class.unwrap_or(&"".into()), &style_state);

                        node.styling.set_texture(
                            &mut style,
                            node.attrs.get("src").unwrap_or(&"".into()),
                            &mut self.tex_manager,
                        );
                        style
                    }
                    "view" => node
                        .styling
                        .set_styling(class.unwrap_or(&"".into()), &style_state),

                    "text" => {
                        let [node, parent] = dom
                            .tree
                            .get_disjoint_node_context_mut([id, parent.unwrap()])
                            .unwrap();

                        let class = node.attrs.get("class");
                        let style = node
                            .styling
                            .set_styling(class.unwrap_or(&"".into()), &style_state);
                        node.styling.text = parent.styling.text.clone();
                        style
                    }

                    _ => unreachable!(),
                };

                let old_style = dom.tree.style(id).unwrap();
                if old_style != &style {
                    dom.tree.set_style(id, style).unwrap();
                }

                true
            });
        }

        fn measure_function(
            known_dimensions: taffy::geometry::Size<Option<f32>>,
            available_space: taffy::geometry::Size<taffy::style::AvailableSpace>,
            node_context: Option<&mut NodeContext>,
            fonts: &Fonts,
        ) -> Size<f32> {
            if let Size {
                width: Some(width),
                height: Some(height),
            } = known_dimensions
            {
                return Size { width, height };
            }

            match node_context {
                None => Size::ZERO,
                Some(node_context) => {
                    if node_context.tag != "text".into() {
                        return Size::ZERO;
                    }

                    let galley = if let AvailableSpace::Definite(space) = available_space.width {
                        fonts.layout(
                            node_context
                                .attrs
                                .get("value")
                                .unwrap_or(&"".into())
                                .to_string(),
                            node_context.styling.text.font.clone(),
                            node_context.styling.text.color,
                            space,
                        )
                    } else {
                        fonts.layout_no_wrap(
                            node_context
                                .attrs
                                .get("value")
                                .unwrap_or(&"".into())
                                .to_string(),
                            node_context.styling.text.font.clone(),
                            node_context.styling.text.color,
                        )
                    };

                    let size = galley.size();
                    node_context.computed.galley = Some(galley);

                    Size {
                        width: size.x,
                        height: size.y,
                    }
                }
            }
        }

        // send event on dirty nodes
        let mut dirty_nodes = vec![];
        dom.traverse_tree(root_id, &mut |dom, id| {
            let is_dirty = dom.tree.dirty(id).unwrap_or(false);
            if is_dirty {
                dirty_nodes.push(id);
            }
            true
        });

        {
            let _guard = tracing::trace_span!("taffy compute layout").entered();
            dom.tree
                .compute_layout_with_measure(
                    root_id,
                    available_space,
                    // Note: this closure is a FnMut closure and can be used to borrow external context for the duration of layout
                    // For example, you may wish to borrow a global font registry and pass it into your text measuring function
                    |known_dimensions, available_space, _node_id, node_context| {
                        measure_function(
                            known_dimensions,
                            available_space,
                            node_context,
                            &self.fonts,
                        )
                    },
                )
                .unwrap();
            self.compute_rects(dom);
        }

        dom.on_layout_changed(&dirty_nodes);
    }

    /// will compute the rects for all the nodes using the final computed layout
    #[tracing::instrument(skip_all, name = "Renderer::compute_rects")]
    pub fn compute_rects(&mut self, dom: &mut Dom) {
        // Now we do a pass so we cache the computed layout in our VDom tree
        let root_id = dom.get_root_id();
        dom.traverse_tree_mut_with_parent_and_data(
            root_id,
            None,
            &Vec2::ZERO,
            &mut |dom, id, parent_id, parent_location_offset| {
                let layout = dom.tree.layout(id).unwrap();

                let parent_scroll_offset = parent_id
                    .map(|parent_id| {
                        let parent_layout = dom.tree.layout(parent_id).unwrap();
                        let parent = dom.tree.get_node_context(parent_id).unwrap();
                        let scroll = parent.scroll;

                        Vec2::new(
                            scroll.x.min(parent_layout.scroll_width()).max(0.0),
                            scroll.y.min(parent_layout.scroll_height()).max(0.0),
                        )
                    })
                    .unwrap_or_default();

                let location = *parent_location_offset - parent_scroll_offset
                    + epaint::Vec2::new(layout.location.x, layout.location.y);

                let rect = epaint::Rect {
                    min: location.to_pos2(),
                    max: Pos2 {
                        x: location.x + layout.size.width,
                        y: location.y + layout.size.height,
                    },
                };

                let node = dom.tree.get_node_context_mut(id).unwrap();
                node.computed.rect = rect;
                (true, location)
            },
        );
    }

    fn get_rect_shape(&self, node: &NodeContext, parent_clip: Rect) -> ClippedShape {
        let styling = &node.styling;
        let rounding = styling.border.radius;
        let rect = epaint::Rect {
            min: epaint::Pos2 {
                x: node.computed.rect.min.x + styling.border.width / 2.0,
                y: node.computed.rect.min.y + styling.border.width / 2.0,
            },
            max: node.computed.rect.max,
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

    #[tracing::instrument(skip_all, name = "Renderer::get_paint_info")]
    pub fn get_paint_info(
        &mut self,
        dom: &mut Dom,
    ) -> (Vec<ClippedPrimitive>, TexturesDelta, &ScreenDescriptor) {
        let now = Instant::now();
        self.calculate_layout(dom);
        log::debug!("layout took: {:?}", now.elapsed());

        // get all computed rects
        let now = Instant::now();
        let root_id = dom.get_root_id();
        let cursor_state = dom.state.cursor_state.clone();
        let selection = dom.state.selection.clone();

        dom.traverse_tree_mut_with_parent_and_data(
            root_id,
            None,
            &None,
            &mut |dom, id, parent_id, parent_clip| {
                let node = dom.tree.get_node_context(id).unwrap();
                let style = dom.tree.style(id).unwrap();

                // we need to make sure the scrollbar doesnt get overwritten
                let node_clip = {
                    epaint::Rect {
                        min: node.computed.rect.min,
                        max: epaint::Pos2 {
                            x: if style.overflow.y == Overflow::Scroll
                                && style.scrollbar_width != 0.0
                            {
                                node.computed.rect.max.x - style.scrollbar_width
                            } else {
                                node.computed.rect.max.x
                            },
                            y: if style.overflow.x == Overflow::Scroll
                                && style.scrollbar_width != 0.0
                            {
                                node.computed.rect.max.y - style.scrollbar_width
                            } else {
                                node.computed.rect.max.y
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
                    _ => {}
                }

                match &(*node.tag) {
                    "text" => {
                        let shape = Shape::galley(
                            node.computed.rect.min,
                            node.computed
                                .galley
                                .clone()
                                .expect("Galley should've been set in the calculate_layout step"),
                            Color32::BLACK,
                        );

                        // if let Some(cursor) = parent.attrs.get("cursor") {
                        //     let epaint::Shape::Text(text_shape) = &shape.shape else {
                        //         unreachable!();
                        //     };
                        //     let Some(selection_start) =
                        //         parent.attrs.get("selection_start").or(Some(cursor))
                        //     else {
                        //         unreachable!();
                        //     };

                        //     if let Ok(cursor) = str::parse::<usize>(cursor) {
                        //         if parent.attrs.get("cursor_visible").unwrap_or(&String::new())
                        //             == "true"
                        //         {
                        //             shapes.push(self.get_cursor_shape(parent, text_shape, cursor));
                        //         }

                        //         if let Ok(selection_start) = str::parse::<usize>(selection_start) {
                        //             shapes.extend_from_slice(&self.get_text_selection_shape(
                        //                 text_shape,
                        //                 cursor,
                        //                 selection_start,
                        //                 parent.styling.text.selection_color,
                        //             ));
                        //         }
                        //     }
                        // }

                        let parent = parent_id
                            .map(|parent_id| dom.tree.get_node_context(parent_id).unwrap());
                        let selection_shapes = self.get_selection_shape(
                            &cursor_state,
                            &selection,
                            &id,
                            node,
                            parent.unwrap(),
                        );
                        self.shapes.extend(selection_shapes);
                        self.shapes.push(ClippedShape {
                            clip_rect: clip,
                            shape,
                        });
                    }
                    _ => {
                        self.shapes.push(self.get_rect_shape(node, clip));

                        let are_both_scrollbars_visible = style.overflow.x == Overflow::Scroll
                            && style.overflow.y == Overflow::Scroll;

                        if style.scrollbar_width > 0.0 && style.overflow.y == Overflow::Scroll {
                            let layout = dom.tree.layout(id).unwrap();
                            let (container_shape, button_shape) = self.get_scrollbar_shape(
                                node,
                                &layout,
                                style.scrollbar_width,
                                false,
                                are_both_scrollbars_visible,
                                false,
                                false,
                            );

                            self.shapes.push(container_shape);
                            self.shapes.push(button_shape);
                        }

                        if style.scrollbar_width > 0.0 && style.overflow.x == Overflow::Scroll {
                            let layout = dom.tree.layout(id).unwrap();
                            let (container_shape, button_shape) = self.get_scrollbar_shape(
                                node,
                                &layout,
                                style.scrollbar_width,
                                true,
                                are_both_scrollbars_visible,
                                false,
                                false,
                            );

                            self.shapes.push(container_shape);
                            self.shapes.push(button_shape);
                        }

                        if are_both_scrollbars_visible {
                            self.shapes.push(self.get_scrollbar_bottom_right_prop(
                                node,
                                &self.shapes[self.shapes.len() - 4],
                                &self.shapes[self.shapes.len() - 2],
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
                self.tex_manager
                    .set(epaint::TextureId::default(), font_image_delta);
            }

            self.tex_manager.take_delta()
        };

        let mut clipped_primitives: Vec<ClippedPrimitive> = Vec::with_capacity(self.shapes.len());
        for clipped_shape in std::mem::take(&mut self.shapes) {
            self.tessellator
                .tessellate_clipped_shape(clipped_shape, &mut clipped_primitives);
        }

        clipped_primitives.retain(|p| {
            p.clip_rect.is_positive()
                && match &p.primitive {
                    Primitive::Mesh(mesh) => !mesh.is_empty(),
                    Primitive::Callback(_) => true,
                }
        });

        log::debug!(
            "paint info took: {:?} - primitives {}",
            now.elapsed(),
            clipped_primitives.len()
        );

        (clipped_primitives, texture_delta, &self.screen_descriptor)
    }

    pub fn get_scrollbar_rect(
        &self,
        node: &NodeContext,
        bar_width: f32,
        horizontal: bool,
        are_both_scrollbars_visible: bool,
    ) -> Rect {
        let styling = &node.styling;
        let location = node.computed.rect.min;
        let size = node.computed.rect.size();

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
        node: &NodeContext,
        layout: &Layout,
        bar_width: f32,
        horizontal: bool,
        are_both_scrollbars_visible: bool,
    ) -> Rect {
        let styling = &node.styling;
        let location = node.computed.rect.min;
        let size = node.computed.rect.size();

        let button_width = bar_width * 0.50; // 50% of bar_width

        if horizontal {
            let thumb_width = (size.x / layout.content_size.width)
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
            let thumb_position_x = (node.scroll.x / layout.scroll_width()) * thumb_max_x;

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
            let thumb_height = (size.y / layout.content_size.height)
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

            let thumb_position_y = (node.scroll.y / layout.scroll_height()) * thumb_max_y;

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
        node: &NodeContext,
        layout: &Layout,
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
                layout,
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
        node: &NodeContext,
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

    pub fn get_selection_shape(
        &mut self,
        cursor_state: &CursorState,
        selection: &Vec<SelectedNode>,
        node_id: &NodeId,
        node: &NodeContext,
        parent: &NodeContext,
    ) -> Vec<ClippedShape> {
        if !cursor_state.drag_start_position.is_some() {
            return vec![];
        }

        let Some(selected_node) = selection.iter().find(|s_node_id| {
            return *node_id == s_node_id.node_id;
        }) else {
            return vec![];
        };

        let parent_clip: Rect = parent.computed.rect;

        let galley = self.fonts.layout(
            node.attrs.get("value").unwrap().clone().to_string(),
            parent.styling.text.font.clone(),
            parent.styling.text.color,
            selected_node.computed_rect_when_selected.size().x + 1.0,
        );

        // println!(
        //     "node.computed.rect: {:?}",
        //     selected_node.computed_rect_when_selected
        // );
        // println!(
        //     " cursor_state.drag_start_position: {:?}",
        //     cursor_state.drag_start_position
        // );
        // println!(
        //     " cursor_state.drag_end_position: {:?}",
        //     cursor_state.drag_end_position
        // );

        let selection_start = cursor_state.drag_start_position.unwrap().to_vec2()
            - selected_node.computed_rect_when_selected.min.to_vec2();
        let selection_end = cursor_state
            .drag_end_position
            .unwrap_or(cursor_state.current_position)
            .to_vec2()
            - selected_node.computed_rect_when_selected.min.to_vec2();

        let start_cursor = galley.cursor_from_pos(selection_start);
        let end_cursor = galley.cursor_from_pos(selection_end);

        let min = start_cursor.rcursor;
        let max = end_cursor.rcursor;

        let mut shapes = vec![];
        for ri in min.row..=max.row {
            let row = &galley.rows[ri];
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
                node.computed.rect.min + vec2(left, row.min_y()),
                node.computed.rect.min + vec2(right, row.max_y()),
            );
            shapes.push(ClippedShape {
                clip_rect: parent_clip,
                shape: epaint::Shape::Rect(epaint::RectShape {
                    rect,
                    rounding: epaint::Rounding::ZERO,
                    fill: parent.styling.text.selection_color,
                    stroke: epaint::Stroke::default(),
                    fill_texture_id: TextureId::default(),
                    uv: epaint::Rect::from_min_max(WHITE_UV, WHITE_UV),
                }),
            });
        }

        shapes
    }
}
