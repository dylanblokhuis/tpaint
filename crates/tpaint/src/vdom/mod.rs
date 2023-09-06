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
use epaint::{text::FontDefinitions, textures::TexturesDelta, ClippedPrimitive, Pos2, Vec2, Rect};
use rustc_hash::{FxHashMap, FxHashSet};
use slotmap::{new_key_type, HopSlotMap};
use smallvec::{smallvec, SmallVec};

use taffy::{Taffy, style::Overflow, prelude::Size};
use winit::{dpi::{PhysicalPosition, PhysicalSize}, event_loop::EventLoopProxy, event::MouseScrollDelta};

use crate::vdom::events::{KeyInput, translate_virtual_key_code};

use self::{tailwind::Tailwind, events::{DomEvent, PointerMove, PointerInput, Focus, Blur, Key, Drag}, renderer::Renderer};

new_key_type! { pub struct NodeId; }

#[derive(Debug)]
pub struct Node {
    pub id: NodeId,
    pub parent_id: Option<NodeId>,
    pub tag: Arc<str>,
    pub attrs: FxHashMap<Arc<str>, String>,
    pub children: SmallVec<[NodeId; MAX_CHILDREN]>,
    pub styling: Tailwind,
    pub scroll: Vec2,
    pub natural_content_size: Size<f32>,
}

#[derive(Debug, Clone, Copy)]
pub struct ScrollNode {
    pub id: NodeId,
    pub scrollbar: Rect,
    pub thumb: Rect,
    pub horizontal: bool,

    pub is_scrollbar_hovered: bool,
    pub is_scrollbar_button_hovered: bool,
    pub is_scrollbar_button_grabbed: bool,
}

pub struct VDom {
    pub nodes: HopSlotMap<NodeId, Node>,
    templates: FxHashMap<String, SmallVec<[NodeId; MAX_CHILDREN]>>,
    stack: SmallVec<[NodeId; MAX_CHILDREN]>,
    element_id_mapping: FxHashMap<ElementId, NodeId>,
    common_tags_and_attr_keys: FxHashSet<Arc<str>>,
    event_listeners: FxHashMap<NodeId, SmallVec<[Arc<str>; 8]>>,
    hovered: SmallVec<[NodeId; MAX_CHILDREN]>,
    focused: Option<NodeId>,
    current_scroll_node: Option<ScrollNode>
}

impl VDom {
    pub fn new() -> VDom {
        let mut nodes = HopSlotMap::with_key();
        let root_id = nodes.insert_with_key(|id| Node {
            id,
            parent_id: None,
            tag: "root".into(),
            attrs: FxHashMap::default(),
            children: smallvec![],
            styling: Tailwind::default(),
            scroll: Vec2::ZERO,
            natural_content_size: Size::ZERO
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
            current_scroll_node: None
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

    pub fn insert_node_before(
        &mut self,
        old_node_id: NodeId,
        new_id: NodeId,
    ) {
        let parent_id = {
            self.nodes[old_node_id].parent_id.unwrap()
        };

        self.nodes[new_id].parent_id = Some(parent_id);
        
        let parent = &mut self.nodes[parent_id];
        
        let index = parent
            .children
            .iter()
            .position(|child| {
                *child == old_node_id
            })
            .unwrap();

        parent.children.insert(index, new_id);
    }

    #[tracing::instrument(skip_all, name = "VDom::apply_mutations")]
    pub fn apply_mutations(&mut self, mutations: Mutations) {
        for template in mutations.templates {
            let mut children = SmallVec::with_capacity(template.roots.len());
            for root in template.roots {
                let id: NodeId = self.create_template_node(root, Some(self.element_id_mapping[&ElementId(0)]));
                children.push(id);
            }
            println!("inserting template {:?}", template.name);
            self.templates.insert(template.name.to_string(), children);
        }

        for edit in mutations.edits {
            match edit {
                dioxus::core::Mutation::LoadTemplate { name, index, id } => {
                    let template_id = self.templates[name][index];
                    let new_id = self.clone_node(template_id, Some(self.element_id_mapping[&ElementId(0)]));
                    self.stack.push(new_id);
                    self.element_id_mapping.insert(id, new_id);
                }
                dioxus::core::Mutation::AssignId { path, id } => {
                    let node_id = self.load_path(path);
                    self.element_id_mapping.insert(id, node_id);
                }
               
                dioxus::core::Mutation::AppendChildren { m, id } => {
                    // println!("stack_len {}", self.stack.len());
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
                        self.nodes[node_id].attrs.remove(name);
                    } else {
                        let key = self.get_tag_or_attr_key(name);
                        self.nodes[node_id].attrs.insert(
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
                    self.nodes[node_id].attrs.insert(key, value.to_string());
                }
                dioxus::core::Mutation::SetText { value, id } => {
                    let node_id = self.element_id_mapping[&id];
                    let key = self.get_tag_or_attr_key("value");
                    self.nodes[node_id].attrs.insert(key, value.to_string());
                }

                dioxus::core::Mutation::ReplaceWith { id, m } => {
                    let new_nodes = self.split_stack(self.stack.len() - m);
                    let old_node_id = self.element_id_mapping[&id];
                    for new in new_nodes {
                        self.insert_node_before(old_node_id, new);
                    }
                    self.remove_node(old_node_id);
                }
                dioxus::core::Mutation::ReplacePlaceholder { path, m } => {
                    let new_nodes = self.split_stack(self.stack.len() - m);
                    let old_node_id = self.load_path(path);

                    for new_id in new_nodes {
                        self.insert_node_before(old_node_id, new_id);
                    }   

                    self.remove_node(old_node_id);
                }
                
                _ => {
                    todo!("unimplemented mutation {:?}", edit);
                }
            }
        }
    }

    /// useful for debugging
    pub fn print_tree(&self, _taffy: &Taffy, id: NodeId, depth: usize) {
        let node = self.nodes.get(id).unwrap();
        match &(*node.tag) {
            "text" => {
                // cut it to 50 chars max
                let ellipsis_text = node.attrs.get("value").unwrap_or(&"".to_string()).chars().take(50).collect::<String>();

                println!("{}{} -> {}", " ".repeat(depth), node.tag, ellipsis_text);
            }
            _ => {
                println!("{}{} -> {}", " ".repeat(depth), node.tag, node.attrs.get("class").unwrap_or(&String::new()));
            }
        }
        for child in node.children.iter() {
            self.print_tree(_taffy,*child, depth + 1);
        }
    }

    fn load_path(&self, path: &[u8]) -> NodeId {
        let mut current_id = *self.stack.last().unwrap();
        let current = &self.nodes[current_id];
        for index in path {
            let new_id = current.children[*index as usize];
            current_id = new_id
        }
        current_id
    }

    #[tracing::instrument(skip_all, name = "VDom::get_tag_or_attr_key")]
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

    #[tracing::instrument(skip_all, name = "VDom::create_template_node")]
    fn create_template_node(&mut self, node: &TemplateNode, parent_id: Option<NodeId>) -> NodeId {
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
                    parent_id,
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
                    scroll: Vec2::ZERO,
                    natural_content_size: Size::ZERO,
                };
                let parent = self.nodes.insert_with_key(|id| {
                    node.id = id;
                    node
                });

                for child in children {
                    let child = self.create_template_node(child, Some(parent));
                    self.nodes[parent].children.push(child);
                }

                parent
            }
            TemplateNode::Text { text } => {
                let mut attrs = FxHashMap::default();
                attrs.insert(self.get_tag_or_attr_key("value"), text.to_string());

                self.nodes.insert_with_key(|id| { Node {
                    id,
                    parent_id,
                    tag: "text".into(),
                    children: smallvec![],
                    attrs,
                    styling: Tailwind::default(),
                    scroll: Vec2::ZERO,
                    natural_content_size: Size::ZERO,
                }})
            }

            TemplateNode::Dynamic { .. } => {
                let mut attrs = FxHashMap::default();
                attrs.insert(self.get_tag_or_attr_key("class"), String::new());

                self.nodes.insert_with_key(|id| { Node {
                    id,
                    parent_id,
                    tag: "view".into(),
                    children: smallvec![],
                    attrs,
                    styling: Tailwind::default(),
                    scroll: Vec2::ZERO,
                    natural_content_size: Size::ZERO,
                }})
            }

            TemplateNode::DynamicText { .. } => {
                let mut attrs = FxHashMap::default();
                attrs.insert(self.get_tag_or_attr_key("value"), String::new());
                self.nodes.insert_with_key(|id| { Node {
                    id,
                    parent_id,
                    tag: "text".into(),
                    children: smallvec![],
                    attrs,
                    styling: Tailwind::default(),
                    scroll: Vec2::ZERO,
                    natural_content_size: Size::ZERO,
                }})
            },
        }
    }

    /// Clone node and its children, they all get new ids
    #[tracing::instrument(skip_all, name = "VDom::clone_node")]
    pub fn clone_node(&mut self, node_id: NodeId, parent_id: Option<NodeId>) -> NodeId {
        let node = &self.nodes[node_id];
        let mut new_node = Node {
            id: NodeId::default(),
            parent_id,
            tag: node.tag.clone(),
            attrs: node.attrs.clone(),
            children: smallvec![],
            styling: node.styling.clone(),
            scroll: Vec2::ZERO,
            natural_content_size: Size::ZERO,
        };
        let new_node_id = self.nodes.insert_with_key(|id| {
            new_node.id = id;
            new_node
        });

        let mut children: [NodeId; MAX_CHILDREN] = [NodeId::default(); MAX_CHILDREN];
        let mut count = 0;

        for child in self.nodes[node_id].children.iter() {
            if count >= MAX_CHILDREN {
                break;
            }
            children[count] = *child;
            count += 1;
        }

        for i in 0..count {
            let id = self.clone_node(children[i], Some(new_node_id));
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

    fn traverse_tree_mut_with_parent_and_data<T>(&mut self, id: NodeId, parent_id: Option<NodeId>, data: &T, callback: &mut impl FnMut(&mut Node, Option<&Node>, &T) -> (bool, T)) {
        let mut children: [NodeId; MAX_CHILDREN] = [NodeId::default(); MAX_CHILDREN];
        let mut count = 0;
        
        let data = { 
            let (node, new_data) = if let Some(parent_id) = parent_id {
                let [node, parent] = self.nodes.get_disjoint_mut([id, parent_id]).unwrap();
                let (should_continue, new_data) = callback(node, Some(parent), data);
                if !should_continue {
                    return;
                }
                
                (node, new_data)
            } else {
                let node: &mut Node = self.nodes.get_mut(id).unwrap();
                let (should_continue, new_data) = callback(node, None, data);
                if !should_continue {
                    return;
                }            
                
                (node, new_data)
            };
            
            for (i, &child_id) in node.children.iter().enumerate() {
                if i >= MAX_CHILDREN {
                    break
                }
                children[i] = child_id;
                count += 1;
            }

            new_data
        };
       
        for i in 0..count {
            self.traverse_tree_mut_with_parent_and_data(children[i], Some(id), &data, callback);
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
            let parent = self.nodes.get_mut(root_id).unwrap_or_else(|| panic!("node with id {:?} not found", root_id));
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

    #[tracing::instrument(skip_all, name = "VDom::remove_node")]
    pub fn remove_node(&mut self, id: NodeId) {
        let parent = { self.nodes[id].parent_id.unwrap() };
        self.traverse_tree_mut(parent, &mut |node| {
            if let Some(index) = node.children.iter().position(|child| *child == id) {
                node.children.remove(index);
                false
            } else {
                true
            }
        });
        self.nodes.remove(id);
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
    cursor: epaint::text::cursor::Cursor,
    drag_start_pos: Pos2,
    is_button_down: bool,
}

#[derive(Clone, Default)]
pub struct KeyboardState {
    pub modifiers: events::Modifiers,
}

pub struct DomEventLoop {
    pub vdom: Arc<Mutex<VDom>>,
    dom_event_sender: tokio::sync::mpsc::UnboundedSender<DomEvent>,

    pub renderer: Renderer,
    pub cursor_state: CursorState,
    pub keyboard_state: KeyboardState,
}

// a node can have a max of 1024 children
const MAX_CHILDREN: usize = 1024;

impl DomEventLoop {
    pub fn spawn<E: Debug + Send + Sync + Clone>(app: fn(Scope) -> Element, window_size: PhysicalSize<u32>, pixels_per_point: f32, event_proxy: EventLoopProxy<E>, redraw_event_to_send: E) -> DomEventLoop {
        let (dom_event_sender, mut dom_event_receiver) = tokio::sync::mpsc::unbounded_channel::<DomEvent>();
        let render_vdom = Arc::new(Mutex::new(VDom::new()));

        #[cfg(all(feature = "hot-reload", debug_assertions))]
        let (hot_reload_tx, mut hot_reload_rx) = tokio::sync::mpsc::unbounded_channel::<dioxus_hot_reload::HotReloadMsg>();
        #[cfg(not(all(feature = "hot-reload", debug_assertions)))]
        let (_, mut hot_reload_rx) = tokio::sync::mpsc::unbounded_channel::<()>();
        
        #[cfg(all(feature = "hot-reload", debug_assertions))]
        dioxus_hot_reload::connect(move |msg| {
            let _ = hot_reload_tx.send(msg);
        });
        
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

            WindowEvent::MouseWheel { delta, phase, .. } => {                
                println!("{:?}", delta);
                
                let mut vdom = self.vdom.lock().unwrap();
                let Some(scroll_node) = vdom.current_scroll_node else {
                    return false;
                };
                let node = &mut vdom.nodes[scroll_node.id];

                let tick_size = 30.0;
                let content_size = node.natural_content_size;
                let viewport_size = self.renderer.taffy.layout(node.styling.node.unwrap()).unwrap().size;
            
                // Calculate the maximum scrollable offsets
                let max_scroll = Vec2::new(
                    content_size.width - viewport_size.width,
                    content_size.height - viewport_size.height
                );
            
                match delta {
                    MouseScrollDelta::LineDelta(x, y) => {      
                        if self.keyboard_state.modifiers.shift {
                            node.scroll -= Vec2::new(*y * tick_size, *x * tick_size);
                        } else {
                            node.scroll -= Vec2::new(*x * tick_size, *y * tick_size);
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
        let mut cursor: (epaint::text::cursor::Cursor, NodeId) = (epaint::text::cursor::Cursor::default(), NodeId::default());
        let mut current_scroll_node = vdom.current_scroll_node;
        let scroll_is_being_dragged = current_scroll_node.map(|s| s.is_scrollbar_button_grabbed).unwrap_or_default();
        vdom.traverse_tree_with_parent_and_data(
            root_id,
            None,
            &Vec2::ZERO,
            &mut |node, parent, parent_location_offset| {
            let Some(node_id) = node.styling.node else {
                return (false, *parent_location_offset);
            };

            let layout = self.renderer.taffy.layout(node_id).unwrap();
            let style = self.renderer.taffy.style(node_id).unwrap();
            let location = *parent_location_offset
                + epaint::Vec2::new(layout.location.x, layout.location.y);
            let node_rect = epaint::Rect {
                min: location.to_pos2(),
                max: Pos2 { x: location.x + layout.size.width, y: location.y + layout.size.height },
            };
            
            if node_rect.contains(translated_mouse_pos)
            {
                elements.push(node.id);       
                if node.tag == "text".into() {
                    cursor = self.get_global_cursor(location, translated_mouse_pos, node, parent.unwrap());
                }     

                // here we figure out if the mouse is hovering over a scrollbar
                if style.overflow.y == Overflow::Scroll && style.scrollbar_width != 0.0 && !scroll_is_being_dragged {
                    let scrollbar = self.renderer.get_scrollbar_rect(node, layout, &location, style.scrollbar_width, false);
                    let thumb = self.renderer.get_scroll_thumb_rect(node, layout, &location, style.scrollbar_width, false);

                    if scrollbar.contains(translated_mouse_pos)
                    {
                        if thumb.contains(translated_mouse_pos) {
                            current_scroll_node = Some(ScrollNode {
                                id: node.id,
                                scrollbar,
                                thumb,
                                is_scrollbar_hovered: true,
                                is_scrollbar_button_hovered: true,
                                is_scrollbar_button_grabbed: false,
                                horizontal: false,
                            });
                        } else {
                            current_scroll_node = Some(ScrollNode {
                                id: node.id,
                                scrollbar,
                                thumb,
                                is_scrollbar_hovered: true,
                                is_scrollbar_button_grabbed: false,
                                is_scrollbar_button_hovered: false,
                                horizontal: false,
                            });
                        }
                    } else {
                        current_scroll_node = Some(ScrollNode {
                            id: node.id,
                            scrollbar,
                            thumb,
                            is_scrollbar_hovered: false,
                            is_scrollbar_button_grabbed: false,
                            is_scrollbar_button_hovered: false,
                            horizontal: false,
                        });
                    }
                }

                if style.overflow.x == Overflow::Scroll && style.scrollbar_width != 0.0 && !scroll_is_being_dragged {
                    let scrollbar = self.renderer.get_scrollbar_rect(node, layout, &location, style.scrollbar_width, true);
                    let thumb = self.renderer.get_scroll_thumb_rect(node, layout, &location, style.scrollbar_width, true);

                    if scrollbar.contains(translated_mouse_pos)
                    {
                        if thumb.contains(translated_mouse_pos) {
                            current_scroll_node = Some(ScrollNode {
                                id: node.id,
                                scrollbar,
                                thumb,
                                is_scrollbar_hovered: true,
                                is_scrollbar_button_hovered: true,
                                is_scrollbar_button_grabbed: false,
                                horizontal: true,
                            });
                        } else {
                            current_scroll_node = Some(ScrollNode {
                                id: node.id,
                                scrollbar,
                                thumb,
                                is_scrollbar_hovered: true,
                                is_scrollbar_button_grabbed: false,
                                is_scrollbar_button_hovered: false,
                                horizontal: true,
                            });
                        }
                    } else {
                        current_scroll_node = Some(ScrollNode {
                            id: node.id,
                            scrollbar,
                            thumb,
                            is_scrollbar_hovered: false,
                            is_scrollbar_button_grabbed: false,
                            is_scrollbar_button_hovered: false,
                            horizontal: true,
                        });
                    }
                }
            }

            (true, location)
        });

        self.cursor_state.cursor = cursor.0;
        vdom.current_scroll_node = current_scroll_node;
        vdom.hovered = elements.clone();
        elements
    }

    pub fn get_global_cursor(&self, location: Vec2, translated_mouse_pos: Pos2,  node: &Node, parent: &Node) -> (epaint::text::cursor::Cursor, NodeId) {    
        let text = node.attrs.get("value").unwrap();
        let galley = node.styling.get_font_galley(text, &self.renderer.taffy, &self.renderer.fonts, &parent.styling);
        let relative_mouse_pos = translated_mouse_pos - location;
        let cursor = galley.cursor_from_pos(relative_mouse_pos.to_vec2());

        (cursor, parent.id)
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
            if self.cursor_state.is_button_down {
                self.send_event_to_element(node_id, "drag", Arc::new(events::Event::Drag(Drag {
                    current_position: pos, 
                    start_position: self.cursor_state.drag_start_pos,
                    cursor_position: self.cursor_state.cursor.pcursor.offset 
                })));
            }
        }

        // handle scroll thumb dragging
        if self.cursor_state.is_button_down {
            let mut vdom = self.vdom.lock().unwrap();
            if let Some(scroll_node) = vdom.current_scroll_node {
                let node = &mut vdom.nodes[scroll_node.id];
                if scroll_node.is_scrollbar_button_grabbed && scroll_node.horizontal {
                    let drag_delta_x = self.cursor_state.last_pos.x - self.cursor_state.drag_start_pos.x;
                    let drag_percentage_x = drag_delta_x / scroll_node.scrollbar.width();

                    let content_width = node.natural_content_size.width;
                    let viewport_width = scroll_node.scrollbar.width();
                    let max_scrollable_distance_x = content_width - viewport_width;
                    node.scroll.x += drag_percentage_x * max_scrollable_distance_x;
                    node.scroll.x = node.scroll.x.clamp(0.0, max_scrollable_distance_x);

                    // Update the drag_start_pos to the current position for the next move event
                    self.cursor_state.drag_start_pos = self.cursor_state.last_pos;
                } else if scroll_node.is_scrollbar_button_grabbed && !scroll_node.horizontal {
                    let drag_delta_y = self.cursor_state.last_pos.y - self.cursor_state.drag_start_pos.y;
                    let drag_percentage_y = drag_delta_y / scroll_node.scrollbar.height();

                    let content_height = node.natural_content_size.height;
                    let viewport_height = scroll_node.scrollbar.height();
                    let max_scrollable_distance_y = content_height - viewport_height;
                    node.scroll.y += drag_percentage_y * max_scrollable_distance_y;
                    node.scroll.y = node.scroll.y.clamp(0.0, max_scrollable_distance_y);

                    // Update the drag_start_pos to the current position for the next move event
                    self.cursor_state.drag_start_pos = self.cursor_state.last_pos;
                }
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
            self.cursor_state.drag_start_pos = self.cursor_state.last_pos;
        }

        {
            let mut vdom = self.vdom.lock().unwrap();
            if let Some(scroll_node) = vdom.current_scroll_node {
                match state {
                    winit::event::ElementState::Pressed => {
                        if scroll_node.thumb.contains(self.cursor_state.last_pos) {
                            vdom.current_scroll_node = Some(ScrollNode {
                                is_scrollbar_button_grabbed: true,
                                ..scroll_node
                            });
                        } else if scroll_node.scrollbar.contains(self.cursor_state.last_pos) {
                            let node = &mut vdom.nodes[scroll_node.id];
                            let style = self.renderer.taffy.style(node.styling.node.unwrap()).unwrap();
                            
                            if style.overflow.y == Overflow::Scroll {
                                let click_y_relative = self.cursor_state.last_pos.y - scroll_node.scrollbar.min.y;
                                let click_percentage = click_y_relative / scroll_node.scrollbar.height();
                
                                let content_height = node.natural_content_size.height;
                                let viewport_height = scroll_node.scrollbar.height();
                
                                let thumb_height = (viewport_height / content_height) * scroll_node.scrollbar.height();
                                let scroll_to_y_centered = click_percentage * (content_height - viewport_height) - (thumb_height / 2.0);
                                let scroll_to_y_final = scroll_to_y_centered.clamp(0.0, content_height - viewport_height);
                                node.scroll.y = scroll_to_y_final;
                            }

                            if style.overflow.x == Overflow::Scroll {
                                let click_x_relative = self.cursor_state.last_pos.x - scroll_node.scrollbar.min.x;
                                let click_percentage_x = click_x_relative / scroll_node.scrollbar.width();
    
                                let content_width = node.natural_content_size.width;
                                let viewport_width = scroll_node.scrollbar.width();
                        
                                let thumb_width = (viewport_width / content_width) * scroll_node.scrollbar.width();
                                let scroll_to_x_centered = click_percentage_x * (content_width - viewport_width) - (thumb_width / 2.0);
                                let scroll_to_x_final = scroll_to_x_centered.clamp(0.0, content_width - viewport_width);
                                node.scroll.x = scroll_to_x_final;
                            }
                        }

                       
                    }
                    winit::event::ElementState::Released => {
                        vdom.current_scroll_node = Some(ScrollNode {
                            is_scrollbar_button_grabbed: false,
                            ..scroll_node
                        });
                    }
                }
            };
        }
        
        
        
        let pressed_data = Arc::new(events::Event::PointerInput(PointerInput { 
            button,
            pos: self.cursor_state.last_pos,
            modifiers: self.keyboard_state.modifiers,
            pressed: true,
            cursor_position:self.cursor_state.cursor.pcursor.offset,
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
            if vdom.nodes[node_id].tag == "text".into() {
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
        self.vdom.lock().unwrap().focused = Some(node_id);
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
