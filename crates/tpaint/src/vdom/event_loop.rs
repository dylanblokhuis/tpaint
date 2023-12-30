use std::{sync::{Arc, Mutex}, fmt::Debug, ops::Deref};

use dioxus::prelude::{ScopeId, VirtualDom, Scope, Element};
use epaint::{Pos2, text::FontDefinitions, ClippedPrimitive, textures::TexturesDelta, Vec2, ClippedShape, Rect, Color32, TextureId, WHITE_UV, vec2, Shape, Fonts, TessellationOptions};
use smallvec::{SmallVec, smallvec};
use taffy::{tree::NodeId, prelude::Size, style::Overflow};
use winit::{dpi::{PhysicalSize, PhysicalPosition}, event_loop::EventLoopProxy, event::MouseScrollDelta};


use super::{
    events::{self, DomEvent, translate_virtual_key_code, Blur, Focus, PointerInput, KeyInput, Drag, PointerMove},
    renderer::{Renderer, ScreenDescriptor},
    taffy_vdom::{Dom, NodeContext, ScrollNode},
    MAX_CHILDREN,
};


#[derive(Clone, Default)]
pub struct CursorState {
    /// the already translated cursor position
    last_pos: Pos2,
    cursor: epaint::text::cursor::Cursor,
    drag_start: (SmallVec<[NodeId; MAX_CHILDREN]>, Pos2),
    is_button_down: bool,
}

#[derive(Clone, Default)]
pub struct KeyboardState {
    pub modifiers: events::Modifiers,
}

pub struct DomEventLoop {
    pub vdom: Arc<Mutex<Dom>>,
    dom_event_sender: tokio::sync::mpsc::UnboundedSender<DomEvent>,
    pub update_scope_sender: tokio::sync::mpsc::UnboundedSender<ScopeId>,

    pub renderer: Renderer,
    pub cursor_state: CursorState,
    pub keyboard_state: KeyboardState,
}

impl DomEventLoop {

    pub fn spawn<E: Debug + Send + Sync + Clone, T: Clone + 'static + Send + Sync>(app: fn(Scope) -> Element, window_size: PhysicalSize<u32>, pixels_per_point: f32, event_proxy: EventLoopProxy<E>, redraw_event_to_send: E, root_context: T) -> DomEventLoop {
        let (dom_event_sender, mut dom_event_receiver) = tokio::sync::mpsc::unbounded_channel::<DomEvent>();
        let render_vdom = Arc::new(Mutex::new(Dom::new()));
    
        #[cfg(all(feature = "hot-reload", debug_assertions))]
        let (hot_reload_tx, mut hot_reload_rx) = tokio::sync::mpsc::unbounded_channel::<dioxus_hot_reload::HotReloadMsg>();
        #[cfg(not(all(feature = "hot-reload", debug_assertions)))]
        let (_, mut hot_reload_rx) = tokio::sync::mpsc::unbounded_channel::<()>();
    
        let (update_scope_sender, mut update_scope_receiver) = tokio::sync::mpsc::unbounded_channel::<ScopeId>();
        
        #[cfg(all(feature = "hot-reload", debug_assertions))]
        dioxus_hot_reload::connect(move |msg| {
            let _ = hot_reload_tx.send(msg);
        });
    
        
        std::thread::spawn({
            let render_vdom = render_vdom.clone();
            move || {
                let mut vdom = VirtualDom::new(app).with_root_context(root_context);
                let mutations = vdom.rebuild();
                dbg!(&mutations);
                render_vdom.lock().unwrap().apply_mutations(mutations);
    
                tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async move {
                    loop {
                        tokio::select! {
                            _ = vdom.wait_for_work() => {},
                            Some(_msg) = hot_reload_rx.recv() => {
                                #[cfg(all(feature = "hot-reload", debug_assertions))]
                                {
                                    match _msg {
                                        dioxus_hot_reload::HotReloadMsg::UpdateTemplate(template) => {
                                            vdom.replace_template(template);
                                        }
                                        dioxus_hot_reload::HotReloadMsg::Shutdown => {
                                            std::process::exit(0);
                                        }
                                    }
                                }                               
                            }
                            Some(event) = dom_event_receiver.recv() => {
                                let DomEvent { name, data, element_id, bubbles } = event;
                                vdom.handle_event(&name, data.deref().clone().into_any(), element_id, bubbles);
                            }
                            Some(scope_id) = update_scope_receiver.recv() => {
                                vdom.get_scope(scope_id).unwrap().needs_update();
                            }
                        }
    
                        let mutations = vdom.render_immediate();
                        render_vdom.lock().unwrap().apply_mutations(mutations);
    
                        event_proxy.send_event(redraw_event_to_send.clone()).unwrap();
                    }
                });
            }
        });
    
        DomEventLoop {
            vdom: render_vdom,
            dom_event_sender,
            update_scope_sender,
            renderer: Renderer::new(window_size, pixels_per_point, FontDefinitions::default()),
            cursor_state: CursorState::default(),
            keyboard_state: KeyboardState::default(),
        }
        
    }

    #[tracing::instrument(skip_all, name = "calculate_layout")]
    pub fn calculate_layout(&self) {
        let start = std::time::Instant::now();
        let mut vdom = self.vdom.lock().unwrap();
        let root_id = vdom.get_root_id();
        let available_space = taffy::prelude::Size {
            width: taffy::style::AvailableSpace::Definite(
                self.renderer.screen_descriptor.size.width as f32 / self.renderer.screen_descriptor.pixels_per_point,
            ),
            height: taffy::style::AvailableSpace::Definite(
                self.renderer.screen_descriptor.size.height as f32 / self.renderer.screen_descriptor.pixels_per_point,
            ),
        };


        let mut font_styling = None;

        vdom.tree.compute_layout_with_measure(root_id,  available_space,
            |known_dimensions, space, node_id, maybe_node_context| {                
                let Some(node_context)  = maybe_node_context else {
                    return Size::ZERO;
                };
                font_styling = Some((node_id, node_context.styling.clone()));
                
                if let Size { width: Some(width), height: Some(height) } = known_dimensions {                
                    // let Some(node_context)  = maybe_node_context else {
                    //     return Size { width, height };
                    // };

                    return Size { width, height };
                } 

                match &*node_context.tag {
                    "text" => {
                        println!("space: {:?} {:?} known dims {:?}", space, node_id, known_dimensions);
                        let text = node_context.attrs.get("value").unwrap().to_string();
                        let Some((_, parent_styling)) =  &font_styling else {
                            unreachable!();
                        };
                        let font_id = parent_styling.text.font.clone();

                        let size = match space.width {
                            taffy::style::AvailableSpace::Definite(width) => {
                                self.renderer.fonts.layout(text, font_id, parent_styling.text.color, width)
                            }
                            taffy::style::AvailableSpace::MaxContent => {
                                self.renderer.fonts.layout_no_wrap(text, font_id, parent_styling.text.color)
                            }
                            taffy::style::AvailableSpace::MinContent => {
                                self.renderer.fonts.layout_no_wrap(text, font_id, parent_styling.text.color)
                            }
                        }.size();

                        Size {
                            width: size.x,
                            height: size.y,
                        }
                    }

                    "view" => {
                        font_styling = Some((node_id, node_context.styling.clone()));

                        Size::ZERO
                    }

                    _ => Size::ZERO,                
                }
            }
        ).unwrap();

        println!("layout took {:?}", start.elapsed());
    }

    pub fn compute_rects(&self) {
        let mut vdom = self.vdom.lock().unwrap();


        fn compute_rects_inner(
            dom: &mut Dom,
            node_id: NodeId,
            parent_id: Option<NodeId>,
            parent_location_offset: Vec2
        ) {

            let parent_scroll_offset = parent_id
                .map(|p| {
                    let parent = dom.tree.get_node_context(p).unwrap();
                    let scroll = parent.scroll;
                    let parent_layout = dom.tree.layout(p).unwrap();

                    Vec2::new(
                        scroll
                            .x
                            .min(parent.natural_content_size.width - parent_layout.size.width)
                            .max(0.0),
                        scroll
                            .y
                            .min(parent.natural_content_size.height - parent_layout.size.height)
                            .max(0.0),
                    )
                })
                .unwrap_or_default();

            let layout = *dom.tree.layout(node_id).unwrap();
            let location = parent_location_offset - parent_scroll_offset
                + epaint::Vec2::new(layout.location.x, layout.location.y);

            let node = dom.tree.get_node_context_mut(node_id).unwrap();
            node.computed_rect = epaint::Rect {
                min: location.to_pos2(),
                max: Pos2 {
                    x: location.x + layout.size.width,
                    y: location.y + layout.size.height,
                },
            };

            for child in dom.tree.children(node_id).unwrap() {
                compute_rects_inner(dom, child, Some(node_id), location);
            }
        }

        let root_id = vdom.get_root_id();
        compute_rects_inner(&mut vdom, root_id, None, Vec2::ZERO);
    }

    
    pub fn get_paint_info(&mut self) -> (Vec<ClippedPrimitive>, TexturesDelta, &ScreenDescriptor) {
        self.calculate_layout();
        self.compute_rects();

        let mut vdom = self.vdom.lock().unwrap();
        let mut shapes: Vec<ClippedShape> = Vec::with_capacity(vdom.tree.total_node_count());

        fn get_shapes_inner(
            dom: &mut Dom,
            node_id: NodeId,
            parent_id: Option<NodeId>,
            parent_clip: Option<Rect>,
            shapes: &mut Vec<ClippedShape>,
            fonts: &Fonts,
        ) {
            let style = dom.tree.style(node_id).unwrap();
            let node = dom.tree.get_node_context(node_id).unwrap();

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
                        clip = node_clip.intersect(current_clip);
                    }
                }
                Overflow::Visible => {
                    if let Some(parent_clip_rect) = parent_clip {
                        clip = parent_clip_rect;
                    }
                }
            }


            match &(*node.tag) {
                "text" => {
                    let parent = dom.tree.get_node_context(parent_id.unwrap()).unwrap();
                    let shape = get_text_shape(fonts, node, parent, clip);

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
                                shapes.push(get_cursor_shape(parent, text_shape, cursor));
                            }

                            if let Ok(selection_start) = str::parse::<usize>(selection_start) {
                                shapes.extend_from_slice(&get_text_selection_shape(
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
                    shapes.push(get_rect_shape(node, clip));
                    let style = dom.tree.style(node_id).unwrap();

                    let are_both_scrollbars_visible = style.overflow.x == Overflow::Scroll
                        && style.overflow.y == Overflow::Scroll;

                    if style.scrollbar_width > 0.0 && style.overflow.y == Overflow::Scroll {
                        let (container, button) = get_scrollbar_shape(
                            node,
                            style.scrollbar_width,
                            false,
                            are_both_scrollbars_visible,
                            dom.current_scroll_node
                                .map(|scroll| {
                                    scroll.is_vertical_scrollbar_hovered && scroll.id == node_id
                                })
                                .unwrap_or(false),
                            dom.current_scroll_node
                                .map(|scroll| {
                                    (scroll.is_vertical_scrollbar_button_hovered
                                        || scroll.is_vertical_scrollbar_button_grabbed)
                                        && scroll.id == node_id
                                })
                                .unwrap_or(false),
                        );
                        shapes.push(container);
                        shapes.push(button);
                    }

                    if style.scrollbar_width > 0.0 && style.overflow.x == Overflow::Scroll {
                        let (container, button) = get_scrollbar_shape(
                            node,
                            style.scrollbar_width,
                            true,
                            are_both_scrollbars_visible,
                            dom.current_scroll_node
                                .map(|scroll| {
                                    scroll.is_horizontal_scrollbar_hovered
                                        && scroll.id == node_id
                                })
                                .unwrap_or(false),
                                dom.current_scroll_node
                                .map(|scroll| {
                                    (scroll.is_horizontal_scrollbar_hovered
                                        || scroll.is_horizontal_scrollbar_button_grabbed)
                                        && scroll.id == node_id
                                })
                                .unwrap_or(false),
                        );
                        shapes.push(container);
                        shapes.push(button);
                    }

                    if are_both_scrollbars_visible {
                        shapes.push(get_scrollbar_bottom_right_prop(
                            node,
                            &shapes[shapes.len() - 4],
                            &shapes[shapes.len() - 2],
                            style.scrollbar_width,
                        ))
                    }
                }
            }

            for child in dom.tree.children(node_id).unwrap() {
                get_shapes_inner(dom, child, Some(node_id), Some(clip), shapes, fonts);
            }
        }

        let root_id = vdom.get_root_id();
        get_shapes_inner(&mut vdom, root_id, None, None, &mut shapes, &self.renderer.fonts);

        let texture_delta = {
            let font_image_delta = self.renderer.fonts.font_image_delta();
            if let Some(font_image_delta) = font_image_delta {
                self.renderer.tex_manager.set(TextureId::default(), font_image_delta);
            }

            self.renderer.tex_manager.take_delta()
        };

        let (font_tex_size, prepared_discs) = {
            let atlas = self.renderer.fonts.texture_atlas();
            let atlas = atlas.lock();
            (atlas.size(), atlas.prepared_discs())
        };

        let primitives = epaint::tessellator::tessellate_shapes(
            self.renderer.fonts.pixels_per_point(),
            TessellationOptions::default(),
            font_tex_size,
            prepared_discs,
            std::mem::take(&mut shapes),
        );
        
        (primitives, texture_delta, &self.renderer.screen_descriptor)
    }

    
    /// bool: whether the window needs to be redrawn
    pub fn on_window_event(&mut self, event: &winit::event::WindowEvent<'_>) -> bool {
        use winit::event::WindowEvent;
        match event {
            WindowEvent::ScaleFactorChanged {
                scale_factor,
                new_inner_size,
            } => {
                self.renderer.screen_descriptor.pixels_per_point = *scale_factor as f32;
                self.renderer.screen_descriptor.size = **new_inner_size;
                true
            }

            WindowEvent::Resized(new_inner_size) => {
                self.renderer.screen_descriptor.size = *new_inner_size;
                true
            }

            WindowEvent::CursorMoved { position, .. } => self.on_mouse_move(position),

            WindowEvent::CursorLeft { .. } => {
                self.cursor_state.last_pos = Pos2::new(0.0, 0.0);
                true
            }

            WindowEvent::ModifiersChanged(state) => {
                self.keyboard_state.modifiers.alt = state.alt();
                self.keyboard_state.modifiers.ctrl = state.ctrl();
                self.keyboard_state.modifiers.shift = state.shift();
                self.keyboard_state.modifiers.mac_cmd = cfg!(target_os = "macos") && state.logo();
                self.keyboard_state.modifiers.command = if cfg!(target_os = "macos") {
                    state.logo()
                } else {
                    state.ctrl()
                };
                
                false
            }

            WindowEvent::KeyboardInput { input, .. } => {
                self.on_keyboard_input(input)
            }

            WindowEvent::MouseWheel { delta,  .. } => {                
                let mut vdom = self.vdom.lock().unwrap();
                let Some(scroll_node) = vdom.current_scroll_node else {
                    return false;
                };
                let node = vdom.tree.get_node_context_mut(scroll_node.id).unwrap();

                let tick_size = 30.0;
                let content_size = node.natural_content_size;
                let viewport_size = self.renderer.taffy.layout(node.styling.node.unwrap()).unwrap().size;
            
                // Calculate the maximum scrollable offsets
                let max_scroll = Vec2::new(
                    content_size.width - viewport_size.width,
                    content_size.height - viewport_size.height
                );
            
                match delta {
                    MouseScrollDelta::LineDelta(_x, y) => {                                             
                        if self.keyboard_state.modifiers.shift {
                            node.scroll.x -= y * tick_size;
                        } else {
                            node.scroll.y -= y * tick_size;
                        }

                    },
                    MouseScrollDelta::PixelDelta(pos) => {
                        node.scroll += Vec2::new(pos.x as f32, pos.y as f32);
                    }
                }
            
                // Clamp the scroll values to ensure they're within acceptable ranges
                node.scroll.x = node.scroll.x.max(0.0).min(max_scroll.x);
                node.scroll.y = node.scroll.y.max(0.0).min(max_scroll.y);

                true
            }

            WindowEvent::ReceivedCharacter(c) => {
                let focused = self.vdom.lock().unwrap().focused;
                if let Some(node_id) = focused {
                    self.send_event_to_element(node_id, "input", Arc::new(events::Event::Text(events::Text {
                        char: *c,
                        modifiers: self.keyboard_state.modifiers,
                    })));
                }
                true
            }

            WindowEvent::Focused(focused) => {
                self.keyboard_state.modifiers = events::Modifiers::default();
                if !focused {
                    self.vdom.lock().unwrap().focused = None;
                }
                false
            }

            WindowEvent::MouseInput { state, button, .. } => {
                self.on_mouse_input(*state, *button)
            }
            
            _ => false,
        }
    }

    pub fn get_elements_on_pos(&mut self, translated_mouse_pos: Pos2) -> SmallVec<[NodeId; MAX_CHILDREN]> {
        let mut vdom = self.vdom.lock().unwrap();
        let root_id = vdom.get_root_id();
        let mut elements = smallvec![];
        // let mut cursor: (epaint::text::cursor::Cursor, NodeId) = (epaint::text::cursor::Cursor::default(), NodeId::default());
        let mut current_scroll_node = vdom.current_scroll_node;
        let is_horizontal_scroll_is_being_dragged = current_scroll_node.map(|s| s.is_horizontal_scrollbar_button_grabbed).unwrap_or_default();
        let is_vertical_scroll_is_being_dragged = current_scroll_node.map(|s| s.is_vertical_scrollbar_button_grabbed).unwrap_or_default();
        let is_any_scrollbar_grabbed = is_horizontal_scroll_is_being_dragged || is_vertical_scroll_is_being_dragged;
        vdom.traverse_tree(
            root_id,
            &mut |(node_id, node)| {
            let Some(node_id) = node.styling.node else {
                return false;
            };

            let style = self.renderer.taffy.style(node_id).unwrap();

            
            if node.computed_rect.contains(translated_mouse_pos)
            {
                elements.push(node_id);       
                

                if !is_any_scrollbar_grabbed && (style.overflow.x == Overflow::Scroll || style.overflow.y == Overflow::Scroll) {
                    current_scroll_node = Some(ScrollNode::new(node_id));
                }

                let are_both_scrollbars_active = style.overflow.x == Overflow::Scroll && style.overflow.y == Overflow::Scroll;

                // here we figure out if the mouse is hovering over a scrollbar
                if style.overflow.y == Overflow::Scroll && style.scrollbar_width != 0.0 && !is_any_scrollbar_grabbed {
                    let scroll_node = current_scroll_node.as_mut().unwrap();
                    scroll_node.set_vertical_scrollbar(
                        get_scrollbar_rect(node,  style.scrollbar_width, false, are_both_scrollbars_active),
                        get_scroll_thumb_rect(node,  style.scrollbar_width, false, are_both_scrollbars_active)
                    );
                }

                if style.overflow.x == Overflow::Scroll && style.scrollbar_width != 0.0 && !is_any_scrollbar_grabbed {
                    let scroll_node = current_scroll_node.as_mut().unwrap();
                    scroll_node.set_horizontal_scrollbar(
                        get_scrollbar_rect(node,  style.scrollbar_width, true, are_both_scrollbars_active),
                        get_scroll_thumb_rect(node,  style.scrollbar_width, true, are_both_scrollbars_active)
                    );
                }
            }

            true
        });

        if let Some(scroll_node) = current_scroll_node.as_mut() {
            let node = vdom.tree.get_node_context_mut(scroll_node.id).unwrap();
            scroll_node.on_mouse_move(&translated_mouse_pos, node.natural_content_size, &mut node.scroll);
        }

        // self.cursor_state.cursor = cursor.0;
        vdom.current_scroll_node = current_scroll_node;
        vdom.hovered = elements.clone();
        elements
    }

    /// finds the first text element on the mouse position and sets the global cursor
    pub fn set_global_cursor(&mut self, mouse_pos: Pos2, specific_nodes: &[NodeId]) {    
        let vdom = self.vdom.clone();
        let mut vdom = vdom.lock().unwrap();
        let root_id = vdom.get_root_id();

        // on input fields you want to select the text on the mouse position, but since the text is not as big as the parent container we need to check this.
        let mut only_parent_of_text_clicked = None;

        vdom.traverse_tree_with_parent(root_id, None, &mut |(node_id, node), parent| {
            if !specific_nodes.is_empty() && !specific_nodes.contains(&node_id) {
                return true;
            }
            

            if node.tag == "text".into() {
                only_parent_of_text_clicked = None;
                let relative_position = mouse_pos.to_vec2() - node.computed_rect.min.to_vec2();
                let text = node.attrs.get("value").unwrap();

                let galley = node.styling.get_font_galley(text, &self.renderer.taffy, &self.renderer.fonts, &parent.unwrap().1.styling);
                let cursor = galley.cursor_from_pos(relative_position);
                self.cursor_state.cursor = cursor;
                return false;
            }

            if node.attrs.get("cursor").is_some() {
                only_parent_of_text_clicked = Some(node_id);

                return true;
            }
            
            true
        });

        if let Some(parent_id) = only_parent_of_text_clicked {
            // let parent = vdom.nodes.get(parent_id).unwrap();
            let parent = vdom.tree.get_node_context(parent_id).unwrap();
            let children = vdom.tree.children(parent_id).unwrap();
            let child_id = children.first().unwrap();
            let node = vdom.tree.get_node_context(*child_id).unwrap();

            let relative_position = mouse_pos.to_vec2() - node.computed_rect.min.to_vec2();
            let text = node.attrs.get("value").unwrap();
            let galley = node.styling.get_font_galley(text, &self.renderer.taffy, &self.renderer.fonts, &parent.styling);
            let cursor = galley.cursor_from_pos(relative_position);
            self.cursor_state.cursor = cursor;
        }
        // picked_node
    }

    fn translate_mouse_pos(&self, pos_in_pixels: &PhysicalPosition<f64>) -> epaint::Pos2 {
        epaint::pos2(
            pos_in_pixels.x as f32 / self.renderer.screen_descriptor.pixels_per_point,
            pos_in_pixels.y as f32 / self.renderer.screen_descriptor.pixels_per_point,
        )
    }

    fn send_event_to_element(&self, node_id: NodeId, listener: &str, event: Arc<events::Event>) {
        let vdom = self.vdom.lock().unwrap();
        let (element_id, _) = vdom.element_id_mapping.iter().find(|(_, id)| **id == node_id).unwrap();
        let Some(listeners) = vdom.event_listeners.get(element_id) else {
            return;
        };

        let Some(name) = listeners.iter().find(|name| (name as &str) == listener) else {
            return;
        };

        self.dom_event_sender.send(DomEvent {
            name: name.clone(),
            data: event.clone(),
            element_id: *element_id,
            bubbles: true,
        }).unwrap();
    }

    fn on_mouse_move(&mut self, pos_in_pixels: &PhysicalPosition<f64>) -> bool {
        let pos = self.translate_mouse_pos(pos_in_pixels);
        self.cursor_state.last_pos = pos;
        let elements = self.get_elements_on_pos(pos);
        
        for node_id in elements {
            self.send_event_to_element(node_id, "mousemove", Arc::new(events::Event::PointerMoved(PointerMove { pos })));
        }

        if self.cursor_state.is_button_down {
           self.set_global_cursor(pos, &self.cursor_state.drag_start.0.clone());

            for node_id in self.cursor_state.drag_start.0.iter() {
                self.send_event_to_element(*node_id, "drag", Arc::new(events::Event::Drag(Drag {
                    current_position: pos, 
                    start_position: self.cursor_state.drag_start.1,
                    cursor_position: self.cursor_state.cursor.pcursor.offset
                })));
            }
            
        }
        true
    }

    fn on_mouse_input(
        &mut self,
        state: winit::event::ElementState,
        button: winit::event::MouseButton,
    ) -> bool {
        let elements = self.get_elements_on_pos(self.cursor_state.last_pos);
        let button = match button {
            winit::event::MouseButton::Left => Some(events::PointerButton::Primary),
            winit::event::MouseButton::Right => Some(events::PointerButton::Secondary),
            winit::event::MouseButton::Middle => Some(events::PointerButton::Middle),
            winit::event::MouseButton::Other(1) => Some(events::PointerButton::Extra1),
            winit::event::MouseButton::Other(2) => Some(events::PointerButton::Extra2),
            winit::event::MouseButton::Other(_) => None,
        };

        let Some(button) = button else {
            return false;
        };
        
        self.cursor_state.is_button_down = state == winit::event::ElementState::Pressed;
        if state == winit::event::ElementState::Pressed {
            self.set_global_cursor(self.cursor_state.last_pos, &elements);
            self.cursor_state.drag_start = (elements.clone(), self.cursor_state.last_pos);
        } else {
            self.cursor_state.drag_start = (smallvec![], Pos2::ZERO);
            self.cursor_state.cursor.pcursor.offset = 0;
        }

        {
            let mut vdom = self.vdom.lock().unwrap();
            if let Some(mut scroll_node) = vdom.current_scroll_node {
                match state {
                    winit::event::ElementState::Pressed => {
                        let node = vdom.tree.get_node_context_mut(scroll_node.id).unwrap();
                        scroll_node.on_click(&self.cursor_state.last_pos, node.natural_content_size, &mut node.scroll);
                    }
                    winit::event::ElementState::Released => {
                        scroll_node.is_horizontal_scrollbar_button_grabbed = false;
                        scroll_node.is_vertical_scrollbar_button_grabbed = false;
                    }
                }

                vdom.current_scroll_node = Some(scroll_node);
            };
        }    

        
        let pressed_data = Arc::new(events::Event::PointerInput(PointerInput { 
            button,
            pos: self.cursor_state.last_pos,
            modifiers: self.keyboard_state.modifiers,
            pressed: true,
            cursor_position: self.cursor_state.cursor.pcursor.offset,
        }));

        let not_pressed_data = Arc::new(events::Event::PointerInput(PointerInput { 
            button,
            pos: self.cursor_state.last_pos,
            modifiers: self.keyboard_state.modifiers,
            pressed: false,
            cursor_position:self.cursor_state.cursor.pcursor.offset,
        }));

        for node_id in elements {
            match state {
                winit::event::ElementState::Pressed => {
                    self.send_event_to_element(node_id, "click", pressed_data.clone());
                    self.send_event_to_element(node_id, "mousedown", pressed_data.clone());
                    self.set_focus(node_id);
                }
                winit::event::ElementState::Released => {
                    self.send_event_to_element(node_id, "mouseup", not_pressed_data.clone());
                }
            }
        }        

        true
    }
    

    fn set_focus(
        &mut self,
        node_id: NodeId,
    ) -> Option<NodeId> {
        // check if its a text node
        {
            let vdom = self.vdom.lock().unwrap();
            let node = vdom.tree.get_node_context(node_id).unwrap();
            if node.tag == "text".into() {
                return None;
            }
        }

        let focused = self.vdom.lock().unwrap().focused;
        if let Some(focused) = focused {
            // it's already focused, so we don't need to do anything
            if focused == node_id {
                return Some(node_id);
            }

            self.send_event_to_element(focused, "blur", Arc::new(events::Event::Blur(Blur)));
        }
        {
            let mut vdom = self.vdom.lock().unwrap();
            vdom.focused = Some(node_id);
        }
        self.send_event_to_element(node_id, "focus", Arc::new(events::Event::Focus(Focus)));
        Some(node_id)
    }


    fn on_keyboard_input(
        &mut self,
        input: &winit::event::KeyboardInput,
    ) -> bool {
        let Some(key) = input.virtual_keycode else {
            return false;
        };

        let Some(key) = translate_virtual_key_code(key) else {
            return false;
        };


        let focused = self.vdom.lock().unwrap().focused;
        if let Some(node_id) = focused {
            match input.state {
                winit::event::ElementState::Pressed => {
                    self.send_event_to_element(node_id, "keydown", Arc::new(events::Event::Key(KeyInput {
                        key,
                        modifiers: self.keyboard_state.modifiers,
                        pressed: true,
                    })));
                }
                winit::event::ElementState::Released => {
                    self.send_event_to_element(node_id, "keyup", Arc::new(events::Event::Key(KeyInput {
                        key,
                        modifiers: self.keyboard_state.modifiers,
                        pressed: false,
                    })));
                }
            }
        }

        false
    }
}



pub fn get_scrollbar_rect(
    node: &NodeContext,
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
    node: &NodeContext,
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
    node: &NodeContext,
    bar_width: f32,
    horizontal: bool,
    are_both_scrollbars_visible: bool,
    hovered: bool,
    thumb_hovered: bool,
) -> (ClippedShape, ClippedShape) {
    let styling = &node.styling;

    let container_shape = epaint::Shape::Rect(epaint::RectShape {
        rect: get_scrollbar_rect(node, bar_width, horizontal, are_both_scrollbars_visible),
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
        rect: get_scroll_thumb_rect(
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

#[tracing::instrument(skip_all, name = "Renderer::get_text_shape")]
fn get_text_shape(fonts: &Fonts, node: &NodeContext, parent_node: &NodeContext, parent_clip: Rect) -> ClippedShape {
    let parent = &parent_node.styling;

    let galley = fonts.layout(
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
fn get_cursor_shape(
    parent: &NodeContext,
    text_shape: &epaint::TextShape,
    cursor_pos: usize,
) -> ClippedShape {
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
            fill: parent.styling.text.color,
            stroke: epaint::Stroke::default(),
            fill_texture_id: TextureId::default(),
            uv: epaint::Rect::from_min_max(WHITE_UV, WHITE_UV),
        }),
    }
}

#[tracing::instrument(skip_all, name = "Renderer::get_cursor_shape")]
fn get_text_selection_shape(
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
fn get_rect_shape(node: &NodeContext, parent_clip: Rect) -> ClippedShape {
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