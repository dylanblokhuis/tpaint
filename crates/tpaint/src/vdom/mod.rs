pub mod events;
pub mod tailwind;
mod renderer;

use std::{
    sync::{Arc, Mutex}, fmt::Debug, ops::Deref,
};

use dioxus::{
    core::{BorrowedAttributeValue, ElementId, Mutations},
    prelude::{Element, Scope, TemplateAttribute, TemplateNode, VirtualDom, ScopeId},
};
use epaint::{text::FontDefinitions, textures::TexturesDelta, ClippedPrimitive, Pos2, Vec2, Rect, ahash::HashSet};
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
    /// absolute positioned rect computed by the layout engine
    pub computed_rect: Rect,
}

#[derive(Debug, Clone, Copy)]
pub struct ScrollNode {
    pub id: NodeId,
    pub vertical_scrollbar: Option<Rect>,
    pub vertical_thumb: Option<Rect>,
    pub horizontal_scrollbar: Option<Rect>,
    pub horizontal_thumb: Option<Rect>,

    pub is_vertical_scrollbar_hovered: bool,
    pub is_horizontal_scrollbar_hovered: bool,
    pub is_vertical_scrollbar_button_hovered: bool,
    pub is_horizontal_scrollbar_button_hovered: bool,
    pub is_vertical_scrollbar_button_grabbed: bool,
    pub is_horizontal_scrollbar_button_grabbed: bool,

    pub thumb_drag_start_pos: Pos2,
}

impl ScrollNode {
    pub fn new(id: NodeId) -> ScrollNode {
        ScrollNode {
            id,
            vertical_scrollbar: None,
            vertical_thumb: None,
            horizontal_scrollbar: None,
            horizontal_thumb: None,
            is_vertical_scrollbar_hovered: false,
            is_horizontal_scrollbar_hovered: false,
            is_vertical_scrollbar_button_hovered: false,
            is_horizontal_scrollbar_button_hovered: false,
            is_vertical_scrollbar_button_grabbed: false,
            is_horizontal_scrollbar_button_grabbed: false,
            
            thumb_drag_start_pos: Pos2::ZERO,
        }
    }

    pub fn set_vertical_scrollbar(&mut self, vertical_scrollbar: Rect, vertical_thumb: Rect) {
        self.vertical_scrollbar = Some(vertical_scrollbar);
        self.vertical_thumb = Some(vertical_thumb);    
    }

    pub fn set_horizontal_scrollbar(&mut self, horizontal_scrollbar: Rect, horizontal_thumb: Rect) {
        self.horizontal_scrollbar = Some(horizontal_scrollbar);
        self.horizontal_thumb = Some(horizontal_thumb);    
    }

    pub fn on_mouse_move(&mut self, mouse_pos: &Pos2, content_size: Size<f32>,  scroll: &mut Vec2) {
        if let Some(vertical_scrollbar) = self.vertical_scrollbar {
            let is_hovered = vertical_scrollbar.contains(*mouse_pos);
            self.is_vertical_scrollbar_hovered = is_hovered;
        }

        if let Some(horizontal_scrollbar) = self.horizontal_scrollbar {
            let is_hovered = horizontal_scrollbar.contains(*mouse_pos);
            self.is_horizontal_scrollbar_hovered = is_hovered;
        }

        if let Some(vertical_thumb) = self.vertical_thumb {
            let is_hovered = vertical_thumb.contains(*mouse_pos);
            self.is_vertical_scrollbar_hovered = is_hovered;
            self.is_vertical_scrollbar_button_hovered = is_hovered;
            let vertical_scrollbar = self.vertical_scrollbar.unwrap();

            if self.is_vertical_scrollbar_button_grabbed {
                let drag_delta_y = mouse_pos.y - self.thumb_drag_start_pos.y;
                let drag_percentage = drag_delta_y / vertical_scrollbar.height();

                let viewport_height = vertical_scrollbar.height();
                let max_scrollable_distance = content_size.height - viewport_height - if let Some(horizontal_scrollbar) = self.horizontal_scrollbar { horizontal_scrollbar.height() } else { 0.0 };

                scroll.y += drag_percentage * max_scrollable_distance;
                scroll.y = scroll.y.clamp(0.0, max_scrollable_distance);

                self.thumb_drag_start_pos = *mouse_pos;
            }
        }

        if let Some(horizontal_thumb) = self.horizontal_thumb {
            let is_hovered = horizontal_thumb.contains(*mouse_pos);
            self.is_horizontal_scrollbar_hovered = is_hovered;
            self.is_horizontal_scrollbar_button_hovered = is_hovered;
            let horizontal_scrollbar = self.horizontal_scrollbar.unwrap();

            if self.is_horizontal_scrollbar_button_grabbed {
                let drag_delta_x = mouse_pos.x - self.thumb_drag_start_pos.x;
                let drag_percentage = drag_delta_x / horizontal_scrollbar.width();

                let viewport_width = horizontal_scrollbar.width();
                let max_scrollable_distance = content_size.width - viewport_width - if let Some(vertical_scrollbar) = self.vertical_scrollbar { vertical_scrollbar.width() } else { 0.0 };

                scroll.x += drag_percentage * max_scrollable_distance;
                scroll.x = scroll.x.clamp(0.0, max_scrollable_distance);

                self.thumb_drag_start_pos = *mouse_pos;
            }
        }
    }

    pub fn on_click(&mut self, mouse_pos: &Pos2, content_size: Size<f32>, scroll: &mut Vec2) {
        if let Some(vertical_scrollbar) = self.vertical_scrollbar {
            let is_hovered = vertical_scrollbar.contains(*mouse_pos);
            self.is_vertical_scrollbar_hovered = is_hovered;
            self.is_vertical_scrollbar_button_hovered = is_hovered && !self.vertical_thumb.unwrap().contains(*mouse_pos);

            if is_hovered {
                let click_y_relative = mouse_pos.y - vertical_scrollbar.min.y;
                let click_percentage = click_y_relative / vertical_scrollbar.height();
                let viewport_height = vertical_scrollbar.height();
    
                let thumb_height = (viewport_height / content_size.height) * vertical_scrollbar.height();
                let scroll_to_y_centered = click_percentage * (content_size.height - viewport_height) - (thumb_height / 2.0);
                let scroll_to_y_final = scroll_to_y_centered.clamp(0.0, content_size.height - viewport_height);
                scroll.y = scroll_to_y_final;
            }
        }

        if let Some(horizontal_scrollbar) = self.horizontal_scrollbar {
            let is_hovered = horizontal_scrollbar.contains(*mouse_pos);
            self.is_horizontal_scrollbar_hovered = is_hovered;
            self.is_horizontal_scrollbar_button_hovered = is_hovered && !self.horizontal_thumb.unwrap().contains(*mouse_pos);

            if is_hovered {
                let click_x_relative = mouse_pos.x - horizontal_scrollbar.min.x;
                let click_percentage = click_x_relative / horizontal_scrollbar.width();
                let viewport_width = horizontal_scrollbar.width();
    
                let thumb_width = (viewport_width / content_size.width) * horizontal_scrollbar.width();
                let scroll_to_x_centered = click_percentage * (content_size.width - viewport_width) - (thumb_width / 2.0);
                let scroll_to_x_final = scroll_to_x_centered.clamp(0.0, content_size.width - viewport_width);
                scroll.x = scroll_to_x_final;
            }
        }

        if let Some(vertical_thumb) = self.vertical_thumb {
            let is_hovered = vertical_thumb.contains(*mouse_pos);
            self.is_vertical_scrollbar_hovered = is_hovered;
            self.is_vertical_scrollbar_button_hovered = is_hovered;
            self.is_vertical_scrollbar_button_grabbed = is_hovered;

            if is_hovered {
                self.thumb_drag_start_pos = *mouse_pos;        
            }
        }

        if let Some(horizontal_thumb) = self.horizontal_thumb {
            let is_hovered = horizontal_thumb.contains(*mouse_pos);
            self.is_horizontal_scrollbar_hovered = is_hovered;
            self.is_horizontal_scrollbar_button_hovered = is_hovered;
            self.is_horizontal_scrollbar_button_grabbed = is_hovered;

            if is_hovered {
                self.thumb_drag_start_pos = *mouse_pos;        
            }
        }
    }
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
    current_scroll_node: Option<ScrollNode>,
    pub dirty_nodes: HashSet<NodeId>,
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
            natural_content_size: Size::ZERO,
            computed_rect: Rect::ZERO,
        });

        let mut element_id_mapping = FxHashMap::default();
        element_id_mapping.insert(ElementId(0), root_id);

        let mut common_tags_and_attr_keys = FxHashSet::default();
        common_tags_and_attr_keys.insert("view".into());
        common_tags_and_attr_keys.insert("class".into());
        common_tags_and_attr_keys.insert("value".into());
        common_tags_and_attr_keys.insert("image".into());

        let mut dirty_nodes = HashSet::default();
        dirty_nodes.insert(root_id);

        VDom {
            nodes,
            templates: FxHashMap::default(),
            stack: smallvec![],
            element_id_mapping,
            common_tags_and_attr_keys,
            event_listeners: FxHashMap::default(),
            hovered: smallvec![],
            focused: None,
            current_scroll_node: None,
            dirty_nodes,
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

        {
            let node = self.nodes.get_mut(new_id).unwrap();
            node.parent_id = Some(parent_id);
        }
        
        let parent = self.nodes.get_mut(parent_id).unwrap();
        
        let index = parent
            .children
            .iter()
            .position(|child| {
                *child == old_node_id
            })
            .unwrap();

        parent.children.insert(index, new_id);
    }

    pub fn insert_node_after(
        &mut self,
        old_node_id: NodeId,
        new_id: NodeId,
    ) {
        let parent_id = {
            self.nodes[old_node_id].parent_id.unwrap()
        };

        {
            let node = self.nodes.get_mut(new_id).unwrap();
            node.parent_id = Some(parent_id);
        }
        
        let parent = self.nodes.get_mut(parent_id).unwrap();
        
        let index = parent
            .children
            .iter()
            .position(|child| {
                *child == old_node_id
            })
            .unwrap();

        parent.children.insert(index + 1, new_id);
    }

    #[tracing::instrument(skip_all, name = "VDom::apply_mutations")]
    pub fn apply_mutations(&mut self, mutations: Mutations) {
        for template in mutations.templates {
            let mut children = SmallVec::with_capacity(template.roots.len());
            for root in template.roots {
                let id: NodeId = self.create_template_node(root, Some(self.element_id_mapping[&ElementId(0)]));
                children.push(id);
                self.dirty_nodes.insert(id);
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
                    self.dirty_nodes.insert(new_id);
                }
                dioxus::core::Mutation::AssignId { path, id } => {
                    let node_id = self.load_path(path);
                    self.element_id_mapping.insert(id, node_id);
                }

                dioxus::core::Mutation::CreatePlaceholder { id } => {
                    let mut node = Node {
                        id: NodeId::default(),
                        parent_id: None,
                        attrs: FxHashMap::default(),
                        children: smallvec![],
                        computed_rect: Rect::ZERO,
                        natural_content_size: Size::ZERO,
                        scroll: Vec2::ZERO,
                        styling: Tailwind::default(),
                        tag: "placeholder".into(),
                    };
                    let node_id = self.nodes.insert_with_key(|id| {
                        node.id = id;
                        node
                    });
                    self.element_id_mapping.insert(id, node_id);
                    self.stack.push(node_id);
                }
               
                dioxus::core::Mutation::AppendChildren { m, id } => {
                    let children = self.split_stack(self.stack.len() - m);
                    let parent = self.element_id_mapping[&id];
                    for child in children {
                        self.nodes[parent].children.push(child);
                    }
                     self.dirty_nodes.insert(parent);
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
                     self.dirty_nodes.insert(node_id);
                }
                dioxus::core::Mutation::CreateTextNode { value, id } => {
                    let mut attrs = FxHashMap::default();
                    attrs.insert(self.get_tag_or_attr_key("value"), value.to_string());

                    let mut node = Node {
                        id: NodeId::default(),
                        parent_id: None,
                        attrs,
                        children: smallvec![],
                        computed_rect: Rect::ZERO,
                        natural_content_size: Size::ZERO,
                        scroll: Vec2::ZERO,
                        styling: Tailwind::default(),
                        tag: "text".into(),
                    };
                    let node_id = self.nodes.insert_with_key(|id| {
                        node.id = id;
                        node
                    });

                    self.element_id_mapping.insert(id, node_id);
                    self.stack.push(node_id);
                }
                dioxus::core::Mutation::HydrateText { path, value, id } => {
                    let node_id = self.load_path(path);
                    let key = self.get_tag_or_attr_key("value");
                    self.element_id_mapping.insert(id, node_id);
                    let node = self.nodes.get_mut(node_id).unwrap();
                    node.attrs.insert(key, value.to_string());
                     self.dirty_nodes.insert(node_id);
                }
                dioxus::core::Mutation::SetText { value, id } => {
                    let node_id = self.element_id_mapping[&id];
                    let key = self.get_tag_or_attr_key("value");
                    let node = self.nodes.get_mut(node_id).unwrap();
                    node.attrs.insert(key, value.to_string());
                     self.dirty_nodes.insert(node_id);
                }

                dioxus::core::Mutation::ReplaceWith { id, m } => {
                    let new_nodes = self.split_stack(self.stack.len() - m);
                    let old_node_id = self.element_id_mapping[&id];
                    for new_id in new_nodes {
                        self.insert_node_before(old_node_id, new_id);
                         self.dirty_nodes.insert(new_id);
                    }
                    self.remove_node(old_node_id);
                }
                dioxus::core::Mutation::ReplacePlaceholder { path, m } => {
                    let new_nodes = self.split_stack(self.stack.len() - m);
                    let old_node_id = self.load_path(path);

                    for new_id in new_nodes {
                        self.insert_node_before(old_node_id, new_id);
                         self.dirty_nodes.insert(new_id);
                    }   

                    self.remove_node(old_node_id);
                }

                dioxus::core::Mutation::InsertAfter { id, m } => {
                    let new_nodes = self.split_stack(self.stack.len() - m);
                    let old_node_id = self.element_id_mapping[&id];
                    for new_id in new_nodes.into_iter().rev() {
                        self.insert_node_after(old_node_id, new_id);
                        self.dirty_nodes.insert(new_id);
                    }
                }

                dioxus::core::Mutation::InsertBefore { id, m } => {
                    let new_nodes = self.split_stack(self.stack.len() - m);
                    let old_node_id = self.element_id_mapping[&id];
                    for new_id in new_nodes {
                        self.insert_node_before(old_node_id, new_id);
                        self.dirty_nodes.insert(new_id);
                    }
                }

                dioxus::core::Mutation::Remove { id } => {
                    let node_id = self.element_id_mapping[&id];
                    self.remove_node(node_id);
                }

                dioxus::core::Mutation::PushRoot { id } => {
                    let node_id = self.element_id_mapping[&id];
                    self.stack.push(node_id);
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
        let mut current = self.nodes.get(*self.stack.last().unwrap()).unwrap();
        for index in path {
            let new_id = current.children[*index as usize];
            current = self.nodes.get(new_id).unwrap();
        }
        current.id
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
                    computed_rect: Rect::ZERO,
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
                    computed_rect: Rect::ZERO,
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
                    computed_rect: Rect::ZERO,
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
                    computed_rect: Rect::ZERO,
                }})
            },
        }
    }

    /// Clone node and its children, they all get new ids
    #[tracing::instrument(skip_all, name = "VDom::clone_node")]
    pub fn clone_node(&mut self, node_id: NodeId, parent_id: Option<NodeId>) -> NodeId {
        let node = self.nodes.get(node_id).unwrap();
        let mut new_node = Node {
            id: NodeId::default(),
            parent_id,
            tag: node.tag.clone(),
            attrs: node.attrs.clone(),
            children: smallvec![],
            styling: node.styling.clone(),
            scroll: Vec2::ZERO,
            natural_content_size: Size::ZERO,
            computed_rect: Rect::ZERO,
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
        let parent = { self.nodes.get(id).unwrap().parent_id.unwrap() };
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
    drag_start: (SmallVec<[NodeId; MAX_CHILDREN]>, Pos2),
    is_button_down: bool,
}

#[derive(Clone, Default)]
pub struct KeyboardState {
    pub modifiers: events::Modifiers,
}

pub struct DomEventLoop {
    pub vdom: Arc<Mutex<VDom>>,
    dom_event_sender: tokio::sync::mpsc::UnboundedSender<DomEvent>,
    pub update_scope_sender: tokio::sync::mpsc::UnboundedSender<ScopeId>,

    pub renderer: Renderer,
    pub cursor_state: CursorState,
    pub keyboard_state: KeyboardState,
}

// a node can have a max of 1024 children
pub const MAX_CHILDREN: usize = 1024;

impl DomEventLoop {
    pub fn spawn<E: Debug + Send + Sync + Clone, T: Clone + 'static + Send + Sync>(app: fn(Scope) -> Element, window_size: PhysicalSize<u32>, pixels_per_point: f32, event_proxy: EventLoopProxy<E>, redraw_event_to_send: E, root_context: T) -> DomEventLoop {
        let (dom_event_sender, mut dom_event_receiver) = tokio::sync::mpsc::unbounded_channel::<DomEvent>();
        let render_vdom = Arc::new(Mutex::new(VDom::new()));

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

    pub fn get_paint_info(&mut self) -> (Vec<ClippedPrimitive>, TexturesDelta, &ScreenDescriptor) {
        let mut vdom = self.vdom.lock().unwrap();
        self.renderer.calculate_layout(&mut vdom);
        self.renderer.compute_rects(&mut vdom);
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

            WindowEvent::MouseWheel { delta,  .. } => {                
                let mut vdom = self.vdom.lock().unwrap();
                let Some(scroll_node) = vdom.current_scroll_node else {
                    return false;
                };
                let node = vdom.nodes.get_mut(scroll_node.id).unwrap();

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
            &mut |node| {
            let Some(node_id) = node.styling.node else {
                return false;
            };

            let style = self.renderer.taffy.style(node_id).unwrap();

            
            if node.computed_rect.contains(translated_mouse_pos)
            {
                elements.push(node.id);       
                

                if !is_any_scrollbar_grabbed && (style.overflow.x == Overflow::Scroll || style.overflow.y == Overflow::Scroll) {
                    current_scroll_node = Some(ScrollNode::new(node.id));
                }

                let are_both_scrollbars_active = style.overflow.x == Overflow::Scroll && style.overflow.y == Overflow::Scroll;

                // here we figure out if the mouse is hovering over a scrollbar
                if style.overflow.y == Overflow::Scroll && style.scrollbar_width != 0.0 && !is_any_scrollbar_grabbed {
                    let scroll_node = current_scroll_node.as_mut().unwrap();
                    scroll_node.set_vertical_scrollbar(
                        self.renderer.get_scrollbar_rect(node,  style.scrollbar_width, false, are_both_scrollbars_active),
                        self.renderer.get_scroll_thumb_rect(node,  style.scrollbar_width, false, are_both_scrollbars_active)
                    );
                }

                if style.overflow.x == Overflow::Scroll && style.scrollbar_width != 0.0 && !is_any_scrollbar_grabbed {
                    let scroll_node = current_scroll_node.as_mut().unwrap();
                    scroll_node.set_horizontal_scrollbar(
                        self.renderer.get_scrollbar_rect(node,  style.scrollbar_width, true, are_both_scrollbars_active),
                        self.renderer.get_scroll_thumb_rect(node,  style.scrollbar_width, true, are_both_scrollbars_active)
                    );
                }
            }

            true
        });

        if let Some(scroll_node) = current_scroll_node.as_mut() {
            scroll_node.on_mouse_move(&translated_mouse_pos, vdom.nodes[scroll_node.id].natural_content_size, &mut vdom.nodes[scroll_node.id].scroll);
        }

        // self.cursor_state.cursor = cursor.0;
        vdom.current_scroll_node = current_scroll_node;
        vdom.hovered = elements.clone();
        elements
    }

    /// finds the first text element on the mouse position and sets the global cursor
    pub fn set_global_cursor(&mut self, mouse_pos: Pos2, specific_nodes: &[NodeId]) {    
        let vdom = self.vdom.clone();
        let  vdom = vdom.lock().unwrap();
        let root_id = vdom.get_root_id();

        // on input fields you want to select the text on the mouse position, but since the text is not as big as the parent container we need to check this.
        let mut only_parent_of_text_clicked = None;

        vdom.traverse_tree_with_parent(root_id, None, &mut |node, parent| {
            if !specific_nodes.is_empty() && !specific_nodes.contains(&node.id) {
                return true;
            }
            

            if node.tag == "text".into() {
                only_parent_of_text_clicked = None;
                let relative_position = mouse_pos.to_vec2() - node.computed_rect.min.to_vec2();
                let text = node.attrs.get("value").unwrap();
                let galley = node.styling.get_font_galley(text, &self.renderer.taffy, &self.renderer.fonts, &parent.unwrap().styling);
                let cursor = galley.cursor_from_pos(relative_position);
                self.cursor_state.cursor = cursor;
                return false;
            }

            if node.attrs.get("cursor").is_some() {
                only_parent_of_text_clicked = Some(node.id);

                return true;
            }
            
            true
        });

        if let Some(parent_id) = only_parent_of_text_clicked {
            let parent = vdom.nodes.get(parent_id).unwrap();
            let node = vdom.nodes.get(*parent.children.first().unwrap()).unwrap();

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
                        scroll_node.on_click(&self.cursor_state.last_pos, vdom.nodes[scroll_node.id].natural_content_size, &mut vdom.nodes[scroll_node.id].scroll);
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
            let node = vdom.nodes.get(node_id).unwrap();
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
            vdom.dirty_nodes.insert(node_id);
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
