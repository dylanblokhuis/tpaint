pub mod events;
pub mod tailwind;
mod renderer;

use std::{
    sync::{Arc, Mutex}, fmt::Debug, ops::Deref,
};

use dioxus::{
    core::{BorrowedAttributeValue, ElementId, Mutations},
    prelude::{Element, Scope, TemplateAttribute, TemplateNode, VirtualDom},
};
use epaint::{text::FontDefinitions, textures::TexturesDelta, ClippedPrimitive, Pos2, Vec2};
use rustc_hash::{FxHashMap, FxHashSet};
use slotmap::{new_key_type, HopSlotMap};
use smallvec::{smallvec, SmallVec};

use taffy::Taffy;
use winit::{dpi::{PhysicalPosition, PhysicalSize}, event_loop::EventLoopProxy};

use crate::vdom::events::{KeyInput, translate_virtual_key_code};

use self::{tailwind::Tailwind, events::{DomEvent, PointerMove, PointerInput, Focus, Blur, Key}, renderer::Renderer};

new_key_type! { pub struct NodeId; }

#[derive(Debug)]
pub struct Node {
    pub id: NodeId,
    pub tag: Arc<str>,
    pub attrs: FxHashMap<Arc<str>, String>,
    pub children: SmallVec<[NodeId; MAX_CHILDREN]>,
    styling: Tailwind,
}

pub struct VDom {
    pub nodes: HopSlotMap<NodeId, Node>,
    templates: FxHashMap<String, SmallVec<[NodeId; MAX_CHILDREN]>>,
    stack: SmallVec<[NodeId; MAX_CHILDREN]>,
    element_id_mapping: FxHashMap<ElementId, NodeId>,
    common_tags_and_attr_keys: FxHashSet<Arc<str>>,
    event_listeners: FxHashMap<NodeId, SmallVec<[Arc<str>; 8]>>,
    hovered: SmallVec<[NodeId; MAX_CHILDREN]>,
    focused: Option<NodeId>
}

impl VDom {
    pub fn new() -> VDom {
        let mut nodes = HopSlotMap::with_key();
        let root_id = nodes.insert_with_key(|id| Node {
            id,
            tag: "root".into(),
            attrs: FxHashMap::default(),
            children: smallvec![],
            styling: Tailwind::default(),
        });

        let mut element_id_mapping = FxHashMap::default();
        element_id_mapping.insert(ElementId(0), root_id);

        let mut common_tags_and_attr_keys = FxHashSet::default();
        common_tags_and_attr_keys.insert("view".into());
        common_tags_and_attr_keys.insert("class".into());
        common_tags_and_attr_keys.insert("value".into());
        common_tags_and_attr_keys.insert("image".into());

        VDom {
            nodes,
            templates: FxHashMap::default(),
            stack: smallvec![],
            element_id_mapping,
            common_tags_and_attr_keys,
            event_listeners: FxHashMap::default(),
            hovered: smallvec![],
            focused: None,
        }
    }

    /// Splits the collection into two at the given index.
    ///
    /// Returns a newly allocated vector containing the elements in the range
    /// `[at, len)`. After the call, the original vector will be left containing
    /// the elements `[0, at)` with its previous capacity unchanged.
    ///
    pub fn split_stack(&mut self, at: usize) -> SmallVec<[NodeId; MAX_CHILDREN]> {
        if at > self.stack.len() {
            let len = self.stack.len();
            panic!("`at` split index (is {at}) should be <= len (is {len})");
        }

        if at == 0 {
            let cap = self.stack.capacity();
            return std::mem::replace(
                &mut self.stack,
                SmallVec::<[NodeId; MAX_CHILDREN]>::with_capacity(cap),
            );
        }

        let other_len = self.stack.len() - at;
        let mut other = SmallVec::<[NodeId; MAX_CHILDREN]>::with_capacity(other_len);

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
                    let template_id = self.templates[name][index];
                    let new_id = self.clone_node(template_id);
                    self.stack.push(new_id);
                    self.element_id_mapping.insert(id, new_id);
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
                dioxus::core::Mutation::HydrateText { path, value, id } => {
                    let node_id = self.load_path(path);
                    let key = self.get_tag_or_attr_key("value");
                    self.element_id_mapping.insert(id, node_id);
                    let node = self.nodes.get_mut(node_id).unwrap();
                    node.attrs.insert(key, value.to_string());
                }
                dioxus::core::Mutation::SetText { value, id } => {
                    let node_id = self.element_id_mapping[&id];
                    let key = self.get_tag_or_attr_key("value");
                    let node = self.nodes.get_mut(node_id).unwrap();
                    node.attrs.insert(key, value.to_string());
                }
                _ => {
                    todo!("unimplemented mutation {:?}", edit);
                }
            }
        }
    }

    /// useful for debugging
    pub fn print_tree(&self, taffy: &Taffy, id: NodeId, depth: usize) {
        let node = self.nodes.get(id).unwrap();
        match &(*node.tag) {
            "text" => {
                if let Some(styling) = node.styling.clone().node {
                    let layout = taffy.layout(styling).unwrap();
                    println!("{}{} -> {} {:?}", " ".repeat(depth), node.tag, node.attrs.get("value").unwrap_or(&"".to_string()), layout);
                } else {
                    println!("{}{} -> {} {:?}", " ".repeat(depth), node.tag, node.attrs.get("value").unwrap_or(&"".to_string()), node.styling);
                }
            }
            _ => {
                if let Some(styling) = node.styling.clone().node {
                    let layout = taffy.layout(styling).unwrap();
                    println!("{}{} -> {} {:?}", " ".repeat(depth), node.tag, node.attrs.get("class").unwrap_or(&String::new()), layout);
                } else {
                    println!("{}{} -> {} {:?}", " ".repeat(depth), node.tag,node.attrs.get("class").unwrap_or(&String::new()), node.styling);
                }
            }
        }
        for child in node.children.iter() {
            self.print_tree(taffy,*child, depth + 1);
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
                                Some((self.get_tag_or_attr_key(name), value.to_string()))
                            } else {
                                None
                            }
                        })
                        .collect(),
                    children: smallvec![],
                    styling: Tailwind::default(),
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
                let mut attrs = FxHashMap::default();
                attrs.insert(self.get_tag_or_attr_key("value"), text.to_string());

                self.nodes.insert_with_key(|id| { Node {
                    id,
                    tag: "text".into(),
                    children: smallvec![],
                    attrs,
                    styling: Tailwind::default(),
                }})
            }

            TemplateNode::Dynamic { .. } => {
                let mut attrs = FxHashMap::default();
                attrs.insert(self.get_tag_or_attr_key("class"), String::new());

                self.nodes.insert_with_key(|id| { Node {
                    id,
                    tag: "view".into(),
                    children: smallvec![],
                    attrs,
                    styling: Tailwind::default(),
                }})
            }

            TemplateNode::DynamicText { .. } => {
                let mut attrs = FxHashMap::default();
                attrs.insert(self.get_tag_or_attr_key("value"), String::new());
                self.nodes.insert_with_key(|id| { Node {
                    id,
                    tag: "text".into(),
                    children: smallvec![],
                    attrs,
                    styling: Tailwind::default(),
                }})
            },
        }
    }

    /// Clone node and its children, they all get new ids
    pub fn clone_node(&mut self, node_id: NodeId) -> NodeId {
        let node = self.nodes.get(node_id).unwrap();
        let mut new_node = Node {
            id: NodeId::default(),
            tag: node.tag.clone(),
            attrs: node.attrs.clone(),
            children: smallvec![],
            styling: node.styling.clone(),
        };
        let new_node_id = self.nodes.insert_with_key(|id| {
            new_node.id = id;
            new_node
        });

        let mut children: [NodeId; MAX_CHILDREN] = [NodeId::default(); MAX_CHILDREN];
        let mut count = 0;

        let node = self.nodes.get(node_id).unwrap();
        for child in node.children.iter() {
            if count >= MAX_CHILDREN {
                break;
            }
            children[count] = *child;
            count += 1;
        }

        for i in 0..count {
            let id = self.clone_node(children[i]);
            self.nodes[new_node_id].children.push(id); 
        }

        new_node_id
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

    fn traverse_tree_with_parent_and_data<T>(
        &self,
        id: NodeId,
        parent_id: Option<NodeId>,
        data: &T,
        callback: &mut impl FnMut(&Node, Option<&Node>, &T) -> (bool, T)
    ) {
        let node = self.nodes.get(id).unwrap();
        let (should_continue, new_data) = callback(node, 
            parent_id.map(|id| self.nodes.get(id).unwrap()),
            data
        );
    
        if !should_continue {
            return;
        }
    
        for child in node.children.iter() {
            self.traverse_tree_with_parent_and_data(*child, Some(id), &new_data, callback);
        }
    }

    fn traverse_tree_mut_with_parent(&mut self, id: NodeId, parent_id: Option<NodeId>, callback: &mut impl FnMut(&mut Node, Option<&Node>) -> bool) {
        let mut children: [NodeId; MAX_CHILDREN] = [NodeId::default(); MAX_CHILDREN];
        let mut count = 0;
        
        { 
            let node = if let Some(parent_id) = parent_id {
                let [node, parent] = self.nodes.get_disjoint_mut([id, parent_id]).unwrap();
                let should_continue = callback(node, Some(parent));
                if !should_continue {
                    return;
                }
                
                node
            } else {
                let node = self.nodes.get_mut(id).unwrap();
                let should_continue: bool = callback(node, None);
                if !should_continue {
                    return;
                }            
                
                node
            };
            
            for (i, &child_id) in node.children.iter().enumerate() {
                if i >= MAX_CHILDREN {
                    break
                }
                children[i] = child_id;
                count += 1;
            }
        }
       
        for i in 0..count {
            self.traverse_tree_mut_with_parent(children[i], Some(id), callback);
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
}

// a node can have a max of 1024 children
const MAX_CHILDREN: usize = 1024;

impl DomEventLoop {
    pub fn spawn<E: Debug + Send + Sync + Clone>(app: fn(Scope) -> Element, window_size: PhysicalSize<u32>, pixels_per_point: f32, event_proxy: EventLoopProxy<E>, redraw_event_to_send: E) -> DomEventLoop {
        let (dom_event_sender, mut dom_event_receiver) =
            tokio::sync::mpsc::unbounded_channel::<DomEvent>();

        let render_vdom = Arc::new(Mutex::new(VDom::new()));

        std::thread::spawn({
            let render_vdom = render_vdom.clone();
            move || {
                let mut vdom = VirtualDom::new(app);
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
            renderer: Renderer::new(window_size, pixels_per_point, FontDefinitions::default()),
            cursor_state: CursorState::default(),
            keyboard_state: KeyboardState::default(),
        }
    }

    pub fn get_paint_info(&mut self) -> (Vec<ClippedPrimitive>, TexturesDelta, &ScreenDescriptor) {
        let mut vdom = self.vdom.lock().unwrap();
        self.renderer.calculate_layout(&mut vdom);
        self.renderer.get_paint_info(&vdom)
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

    pub fn get_elements_on_pos(&self, translated_mouse_pos: Pos2) -> SmallVec<[NodeId; MAX_CHILDREN]> {
        let mut vdom = self.vdom.lock().unwrap();
        let root_id = vdom.get_root_id();
        let mut elements = smallvec![];
        
        vdom.traverse_tree_with_parent_and_data(
            root_id,
            None,
            &Vec2::ZERO,
            &mut |node, _parent, parent_location_offset| {
            let Some(node_id) = node.styling.node else {
                return (false, *parent_location_offset);
            };

            let layout = self.renderer.taffy.layout(node_id).unwrap();
            let location = *parent_location_offset
                + epaint::Vec2::new(layout.location.x, layout.location.y);

            if translated_mouse_pos.x >= location.x
                && translated_mouse_pos.x <= location.x + layout.size.width
                && translated_mouse_pos.y >= location.y
                && translated_mouse_pos.y <= location.y + layout.size.height
            {
                elements.push(node.id);            
            }

            (true, location)
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
                    self.set_focus(node_id);
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

        true
    }

    fn set_focus(
        &mut self,
        node_id: NodeId,
    ) {
        // check if its a text node
        {
            let vdom = self.vdom.lock().unwrap();
            let node = vdom.nodes.get(node_id).unwrap();
            if node.tag == "text".into() {
                return;
            }
        }

        let focused = self.vdom.lock().unwrap().focused;
        if let Some(focused) = focused {
            // it's already focused, so we don't need to do anything
            if focused == node_id {
                return;
            }

            self.send_event_to_element(focused, "blur", Arc::new(events::Event::Blur(Blur)));
        }
        self.vdom.lock().unwrap().focused = Some(node_id);
        self.send_event_to_element(node_id, "focus", Arc::new(events::Event::Focus(Focus)));
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

        if key == Key::F12 && input.state == winit::event::ElementState::Pressed{
            self.debug_print_tree();
        }

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

    pub fn debug_print_tree(&self) {
        let vdom = self.vdom.lock().unwrap();
        vdom.print_tree(&self.renderer.taffy, vdom.get_root_id(), 0);
    }
}
