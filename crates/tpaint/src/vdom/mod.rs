pub mod events;
pub mod tailwind;

use std::{
    sync::{Arc, Mutex}, fmt::Debug, ops::Deref,
};

use dioxus::{
    core::{BorrowedAttributeValue, ElementId, Mutations},
    prelude::{Element, Scope, TemplateAttribute, TemplateNode, VirtualDom},
};
use epaint::{ClippedShape, TextureId, WHITE_UV, Fonts, TextureManager, text::FontDefinitions, textures::{TextureOptions, TexturesDelta}, TessellationOptions, ClippedPrimitive, Tessellator, Pos2};
use rustc_hash::{FxHashMap, FxHashSet};
use slotmap::{new_key_type, HopSlotMap};
use smallvec::{smallvec, SmallVec};
use taffy::{Taffy, prelude::Size};
use winit::{dpi::{PhysicalPosition, PhysicalSize}, event_loop::EventLoopProxy};

use self::{tailwind::{Tailwind, StyleState}, events::{DomEvent, PointerMove, PointerInput}};

new_key_type! { pub struct NodeId; }

#[derive(Debug)]
pub struct Node {
    pub id: NodeId,
    pub tag: Arc<str>,
    pub attrs: FxHashMap<Arc<str>, String>,
    pub children: SmallVec<[NodeId; 32]>,
    styling: Option<Tailwind>,
}

pub struct VDom {
    pub nodes: HopSlotMap<NodeId, Node>,
    templates: FxHashMap<String, SmallVec<[NodeId; 32]>>,
    stack: SmallVec<[NodeId; 32]>,
    element_id_mapping: FxHashMap<ElementId, NodeId>,
    common_tags_and_attr_keys: FxHashSet<Arc<str>>,
    event_listeners: FxHashMap<NodeId, SmallVec<[Arc<str>; 8]>>,
    hovered: SmallVec<[NodeId; MAX_CHILDREN]>,
}

impl VDom {
    pub fn new() -> VDom {
        let mut nodes = HopSlotMap::with_key();
        let root_id = nodes.insert_with_key(|id| Node {
            id,
            tag: "root".into(),
            attrs: FxHashMap::default(),
            children: smallvec![],
            styling: None,
        });

        let mut element_id_mapping = FxHashMap::default();
        element_id_mapping.insert(ElementId(0), root_id);

        let mut common_tags_and_attr_keys = FxHashSet::default();
        common_tags_and_attr_keys.insert("view".into());
        common_tags_and_attr_keys.insert("class".into());
        common_tags_and_attr_keys.insert("value".into());

        VDom {
            nodes,
            templates: FxHashMap::default(),
            stack: smallvec![],
            element_id_mapping,
            common_tags_and_attr_keys,
            event_listeners: FxHashMap::default(),
            hovered: smallvec![],
        }
    }

    /// Splits the collection into two at the given index.
    ///
    /// Returns a newly allocated vector containing the elements in the range
    /// `[at, len)`. After the call, the original vector will be left containing
    /// the elements `[0, at)` with its previous capacity unchanged.
    ///
    pub fn split_stack(&mut self, at: usize) -> SmallVec<[NodeId; 32]> {
        if at > self.stack.len() {
            let len = self.stack.len();
            panic!("`at` split index (is {at}) should be <= len (is {len})");
        }

        if at == 0 {
            let cap = self.stack.capacity();
            return std::mem::replace(
                &mut self.stack,
                SmallVec::<[NodeId; 32]>::with_capacity(cap),
            );
        }

        let other_len = self.stack.len() - at;
        let mut other = SmallVec::<[NodeId; 32]>::with_capacity(other_len);

        unsafe {
            self.stack.set_len(at);
            other.set_len(other_len);

            std::ptr::copy_nonoverlapping(
                self.stack.as_ptr().add(at),
                other.as_mut_ptr(),
                other_len,
            );
        }

        other
    }

    pub fn apply_mutations(&mut self, mutations: Mutations) {
        for template in mutations.templates {
            let mut children = SmallVec::with_capacity(template.roots.len());
            for root in template.roots {
                let id: NodeId = self.create_template_node(root);
                children.push(id);
            }
            println!("inserting template {:?}", template.name);
            self.templates.insert(template.name.to_string(), children);
        }

        for edit in mutations.edits {
            match edit {
                dioxus::core::Mutation::LoadTemplate { name, index, id } => {
                    println!("{} {}", name, index);
                    println!("{:?}", self.templates.keys());

                    let template_id = self.templates[name][index];
                    self.stack.push(template_id);
                    self.element_id_mapping.insert(id, template_id);
                }
                dioxus::core::Mutation::AssignId { path, id } => {
                    let node_id = self.load_path(path);
                    self.element_id_mapping.insert(id, node_id);
                }
                dioxus::core::Mutation::ReplacePlaceholder { path, m } => {
                    let new_nodes = self.split_stack(self.stack.len() - m);
                    let old_node_id = self.load_path(path);
                    let node = self.nodes.get_mut(old_node_id).unwrap();
                    node.children = new_nodes;
                }
                dioxus::core::Mutation::AppendChildren { m, id } => {
                    let children = self.split_stack(self.stack.len() - m);
                    println!("finding in map {:?}", id);
                    let parent = self.element_id_mapping[&id];
                    for child in children {
                        self.nodes[parent].children.push(child);
                    }
                }
                dioxus::core::Mutation::NewEventListener { name, id } => {
                    let name = self.get_tag_or_attr_key(name);
                    let node_id = self.element_id_mapping[&id];
                    if let Some(listeners) = self.event_listeners.get_mut(&node_id) {
                        listeners.push(name);
                    } else {
                        self.event_listeners.insert(node_id, smallvec![name]);
                    }
                }
                dioxus::core::Mutation::RemoveEventListener { name, id } => {
                    let name = self.get_tag_or_attr_key(name);
                    let node_id = self.element_id_mapping[&id];
                    if let Some(listeners) = self.event_listeners.get_mut(&node_id) {
                        listeners.retain(|val| val != &name);
                    }
                }
                dioxus::core::Mutation::SetAttribute {
                    name, value, id, ..
                } => {
                    let node_id = self.element_id_mapping[&id];
                    if let BorrowedAttributeValue::None = &value {
                        let node = self.nodes.get_mut(node_id).unwrap();
                        node.attrs.remove(name);
                    } else {
                        let key = self.get_tag_or_attr_key(name);
                        let node = self.nodes.get_mut(node_id).unwrap();
                        node.attrs.insert(
                            key,
                            match value {
                                BorrowedAttributeValue::Int(val) => val.to_string(),
                                BorrowedAttributeValue::Bool(val) => val.to_string(),
                                BorrowedAttributeValue::Float(val) => val.to_string(),
                                BorrowedAttributeValue::Text(val) => val.to_string(),
                                BorrowedAttributeValue::None => "".to_string(),
                                BorrowedAttributeValue::Any(_) => unimplemented!(),
                            },
                        );
                    }
                }
                _ => {}
            }
        }
    }

    fn load_path(&self, path: &[u8]) -> NodeId {
        let mut current_id = *self.stack.last().unwrap();
        let current = self.nodes.get(current_id).unwrap();
        for index in path {
            let new_id = current.children[*index as usize];
            current_id = new_id
        }
        current_id
    }

    pub fn get_tag_or_attr_key(&mut self, key: &str) -> Arc<str> {
        if let Some(s) = self.common_tags_and_attr_keys.get(key) {
            s.clone()
        } else {
            let key: Arc<str> = key.into();
            let r = key.clone();
            self.common_tags_and_attr_keys.insert(key);
            r
        }
    }

    fn create_template_node(&mut self, node: &TemplateNode) -> NodeId {
        match *node {
            TemplateNode::Element {
                tag,
                attrs,
                children,
                ..
            } => {
                let mut node = Node {
                    // will instantly be overwritten by insert_with_key
                    id: NodeId::default(),
                    tag: self.get_tag_or_attr_key(tag),
                    attrs: attrs
                        .iter()
                        .filter_map(|val| {
                            if let TemplateAttribute::Static { name, value, .. } = val {
                                println!("static attr {:?}", name);
                                Some((self.get_tag_or_attr_key(name), value.to_string()))
                            } else {
                                None
                            }
                        })
                        .collect(),
                    children: smallvec![],
                    styling: None,
                };
                let parent = self.nodes.insert_with_key(|id| {
                    node.id = id;
                    node
                });

                for child in children {
                    let child = self.create_template_node(child);
                    self.nodes[parent].children.push(child);
                }

                parent
            }
            TemplateNode::Text { text } => {
                let mut map = FxHashMap::default();
                map.insert(self.get_tag_or_attr_key("value"), text.to_string());

                self.nodes.insert_with_key(|id| { Node {
                    id,
                    tag: "text".into(),
                    children: smallvec![],
                    attrs: map,
                    styling: None,
                }})
            }

            _ => self.nodes.insert_with_key(|id| { Node {
                id,
                tag: "placeholder".into(),
                children: smallvec![],
                attrs: FxHashMap::default(),
                styling: None,
            }}),
        }
    }

    pub fn get_root_id(&self) -> NodeId {
        self.element_id_mapping[&ElementId(0)]
    }

    fn traverse_tree(&self, id: NodeId, callback: &mut impl FnMut(&Node) -> bool) {
        let node = self.nodes.get(id).unwrap();
        let should_continue = callback(node);
        if !should_continue {
            return;
        }
        for child in node.children.iter() {
            self.traverse_tree(*child,  callback);
        }
    }
    
    fn traverse_tree_with_parent(&self, id: NodeId, parent_id: Option<NodeId>, callback: &mut impl FnMut(&Node, Option<&Node>) -> bool) {
        let node = self.nodes.get(id).unwrap();
        let should_continue = callback(node, 
            parent_id.map(|id| self.nodes.get(id).unwrap())
        );

        if !should_continue {
            return;
        }

        for child in node.children.iter() {
            self.traverse_tree_with_parent(*child, Some(id), callback);
        }
    }

    pub fn traverse_tree_mut(&mut self, root_id: NodeId, callback: &mut impl FnMut(&mut Node) -> bool) {
        let mut children: [NodeId; MAX_CHILDREN] = [NodeId::default(); MAX_CHILDREN];
        let mut count = 0;
    
        {
            let parent = self.nodes.get_mut(root_id).unwrap();
            let should_continue = callback(parent);

            if !should_continue {
                return;
            }
            
            for (i, &child_id) in parent.children.iter().enumerate() {
                if i >= MAX_CHILDREN {
                    break
                }
                children[i] = child_id;
                count += 1;
            }
        }
    
        for i in 0..count {
            self.traverse_tree_mut(children[i], callback);
        }
    }
}

#[derive(Clone, Debug)]
pub struct ScreenDescriptor {
    pub pixels_per_point: f32,
    pub size: PhysicalSize<u32>,
}

pub struct Renderer {
    pub screen_descriptor: ScreenDescriptor,
    pub fonts: Fonts,
    pub tex_manager: TextureManager,
}

impl Renderer {
    pub fn new(window_size: PhysicalSize<u32>, definitions: FontDefinitions) -> Renderer {
        let fonts = Fonts::new(1.0, 1024, definitions);
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
            screen_descriptor: ScreenDescriptor { pixels_per_point: 1.0, size: window_size },
            fonts,
            tex_manager
        }
    }
}

#[derive(Clone, Default)]
pub struct CursorState {
    /// the already translated cursor position
    last_pos: Pos2,
}

#[derive(Clone, Default)]
pub struct KeyboardState {
    pub modifiers: events::Modifiers,
}

pub struct DomEventLoop {
    vdom: Arc<Mutex<VDom>>,
    dom_event_sender: tokio::sync::mpsc::UnboundedSender<DomEvent>,

    pub renderer: Renderer,
    pub cursor_state: CursorState,
    pub keyboard_state: KeyboardState,
    taffy: Taffy,
}

// a node can have a max of 1024 children
const MAX_CHILDREN: usize = 1024;

impl DomEventLoop {
    pub fn spawn<E: Debug + Send + Sync + Clone>(app: fn(Scope) -> Element, window_size: PhysicalSize<u32>, event_proxy: EventLoopProxy<E>, redraw_event_to_send: E) -> DomEventLoop {
        let (dom_event_sender, mut dom_event_receiver) =
            tokio::sync::mpsc::unbounded_channel::<DomEvent>();

        let render_vdom = Arc::new(Mutex::new(VDom::new()));

        std::thread::spawn({
            let render_vdom = render_vdom.clone();
            move || {
                let mut vdom = VirtualDom::new(app);
                let mutations = vdom.rebuild();
                // dbg!(&mutations);
                render_vdom.lock().unwrap().apply_mutations(mutations);

                tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async move {
                    loop {
                        tokio::select! {
                            _ = vdom.wait_for_work() => {},
                            Some(event) = dom_event_receiver.recv() => {
                                let DomEvent { name, data, element_id, bubbles } = event;
                                vdom.handle_event(&name, data.deref().clone().into_any(), element_id, bubbles);
                            }
                        }

                        let mutations = vdom.render_immediate();
                        // dbg!(&mutations);
                        render_vdom.lock().unwrap().apply_mutations(mutations);
                        
                        event_proxy.send_event(redraw_event_to_send.clone()).unwrap();
                    }
                });
            }
        });

        DomEventLoop {
            vdom: render_vdom,
            dom_event_sender,
            renderer: Renderer::new(window_size, FontDefinitions::default()),
            cursor_state: CursorState::default(),
            keyboard_state: KeyboardState::default(),
            taffy: Taffy::new(),
        }
    }

    pub fn calculate_layout(&mut self) {
        let mut vdom = self.vdom.lock().unwrap();
        let root_id = vdom.get_root_id();
        
        // give root_node styling
        {
            vdom.nodes.get_mut(root_id).unwrap().attrs.insert("class".into(), "w-full h-full".into());
        }

        let taffy = &mut self.taffy;
        let hovered = vdom.hovered.clone();
        vdom.traverse_tree_mut(root_id, &mut |node| {
            let Some(class_attr) = node.attrs.get("class") else {
                return true;
            };

            if let Some(styling) = &mut node.styling {
                styling.set_styling(taffy, class_attr, &StyleState {
                    hovered: hovered.contains(&node.id),
                    focused: false,
                });
            } else {
                let mut tw = Tailwind::default();
                tw.set_styling(taffy, class_attr, &StyleState {
                    hovered: hovered.contains(&node.id),
                    focused: false,
                });
                node.styling = Some(tw);
            }

            return true;
        });

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
            return true;
        });

        let node = vdom.nodes.get(root_id).unwrap();
        let styling = node.styling.as_ref().unwrap();
        taffy.compute_layout(styling.node.unwrap(), Size {
            width: taffy::style::AvailableSpace::Definite(self.renderer.screen_descriptor.size.width as f32),
            height: taffy::style::AvailableSpace::Definite(self.renderer.screen_descriptor.size.height as f32),
        }).unwrap()
    }

    pub fn get_paint_info(&mut self) -> (Vec<ClippedPrimitive>, TexturesDelta, &ScreenDescriptor) {
        self.calculate_layout();

        let root_id = self.vdom.lock().unwrap().get_root_id();
        let vdom = self.vdom.lock().unwrap();
        // perhaps use VecDeque?
        let mut shapes = Vec::with_capacity(vdom.nodes.len());
        vdom.traverse_tree_with_parent(root_id, None,&mut |node, parent| {
            let Some(styling) = &node.styling else {
                return true;
            };
            let node = styling.node.unwrap();
            let layout = self.taffy.layout(node).unwrap();

            let location = if let Some(parent) = parent {
                let parent_layout = self.taffy.layout(parent.styling.as_ref().unwrap().node.unwrap()).unwrap();
                epaint::Vec2::new(parent_layout.location.x, parent_layout.location.y) + epaint::Vec2::new(layout.location.x, layout.location.y)
            } else {
                epaint::Vec2::new(layout.location.x, layout.location.y)
            };

            let width: f32 = layout.size.width;
            let height: f32 = layout.size.height;
            // let border_width = if focused {
            //     FOCUS_BORDER_WIDTH
            // } else {
            //     tailwind.border.width
            // };
            let border_width = styling.border.width;
            let rounding = styling.border.radius;
            let x_start = location.x + border_width / 2.0;
            let y_start = location.y + border_width / 2.0;
            let x_end: f32 = location.x + width - border_width / 2.0;
            let y_end: f32 = location.y + height - border_width / 2.0;
            let rect = epaint::Rect {
                min: epaint::Pos2 {
                    x: x_start,
                    y: y_start,
                },
                max: epaint::Pos2 { x: x_end, y: y_end },
            };
            
            let shape =  epaint::Shape::Rect(epaint::RectShape {
                rect,
                rounding,
                fill: styling.background_color,
                stroke: epaint::Stroke {
                    width: border_width,
                    color: styling.border.color,
                },
                fill_texture_id: TextureId::default(),
                uv: epaint::Rect::from_min_max(WHITE_UV, WHITE_UV),
            });
            let clip = shape.visual_bounding_rect();

            shapes.push(ClippedShape {
                clip_rect: clip,
                shape,
            });    

            return true;            
        });

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

        let primitives = {
            epaint::tessellator::tessellate_shapes(
                self.renderer.fonts.pixels_per_point(),
                TessellationOptions::default(),
                font_tex_size,
                prepared_discs,
                std::mem::take(&mut shapes),
            )
        };

        (
            primitives,
            texture_delta,
            &self.renderer.screen_descriptor
        )    
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
                false
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

            WindowEvent::Focused(focused) => {
                self.keyboard_state.modifiers = events::Modifiers::default();
                false
            }

            WindowEvent::MouseInput { state, button, .. } => {
                self.on_mouse_input(*state, *button)
            }
            

            // WindowEvent::MouseInput { state, button, .. } => {}
            _ => false,
        }
    }

    pub fn get_elements_by_event(&self, event_listener: &str) {
        // self.event_
    }

    pub fn get_elements_on_pos(&self, translated_mouse_pos: Pos2) -> SmallVec<[NodeId; MAX_CHILDREN]> {
        let mut vdom = self.vdom.lock().unwrap();
        let root_id = vdom.get_root_id();
        let mut elements = smallvec![];
        vdom.traverse_tree_with_parent(root_id, None, &mut |node, parent| {
            let Some(styling) = &node.styling else {
                return true;
            };
            let node_id = styling.node.unwrap();
            let layout = self.taffy.layout(node_id).unwrap();
            let absolute_location = if let Some(parent) = parent {
                let parent_layout = self.taffy.layout(parent.styling.as_ref().unwrap().node.unwrap()).unwrap();
                epaint::Vec2::new(parent_layout.location.x, parent_layout.location.y) + epaint::Vec2::new(layout.location.x, layout.location.y)
            } else {
                epaint::Vec2::new(layout.location.x, layout.location.y)
            };

            if translated_mouse_pos.x >= absolute_location.x
                && translated_mouse_pos.x <= absolute_location.x + layout.size.width
                && translated_mouse_pos.y >= absolute_location.y
                && translated_mouse_pos.y <= absolute_location.y + layout.size.height
            {
                elements.push(node.id);            
            }

            return true;
        });

        vdom.hovered = elements.clone();
        elements
    }

    fn translate_mouse_pos(&self, pos_in_pixels: &PhysicalPosition<f64>) -> epaint::Pos2 {
        epaint::pos2(
            pos_in_pixels.x as f32 / self.renderer.screen_descriptor.pixels_per_point,
            pos_in_pixels.y as f32 / self.renderer.screen_descriptor.pixels_per_point,
        )
    }

    fn send_event_to_element(&mut self, node_id: NodeId, listener: &str, event: Arc<events::Event>) {
        let vdom = self.vdom.lock().unwrap();
        let Some(listeners) = vdom.event_listeners.get(&node_id) else {
            return;
        };

        let Some(name) = listeners.iter().find(|name| (name as &str) == listener) else {
            return;
        };

        let (id, _) = vdom.element_id_mapping.iter().find(|(_, id)| **id == node_id).unwrap();
        self.dom_event_sender.send(DomEvent {
            name: name.clone(),
            data: event.clone(),
            element_id: *id,
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

        // dioxus will request redraws so we don't need to
        false
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
        
        for node_id in elements {
            match state {
                winit::event::ElementState::Pressed => {
                    let data = Arc::new(events::Event::PointerInput(PointerInput { 
                        button,
                        pos: self.cursor_state.last_pos,
                        modifiers: self.keyboard_state.modifiers,
                        pressed: true,
                    }));
                    self.send_event_to_element(node_id, "click", data.clone());
                    self.send_event_to_element(node_id, "mousedown", data);
                }
                winit::event::ElementState::Released => {
                    self.send_event_to_element(node_id, "mouseup", Arc::new(events::Event::PointerInput(PointerInput { 
                        button,
                        pos: self.cursor_state.last_pos,
                        modifiers: self.keyboard_state.modifiers,
                        pressed: false,
                    })));
                }
            }
        }

        false
    }
}
