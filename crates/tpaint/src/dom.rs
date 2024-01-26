use std::{sync::Arc, time::Instant};

use dioxus::{
    core::{BorrowedAttributeValue, ElementId, Mutations},
    prelude::{TemplateAttribute, TemplateNode},
};
use epaint::{text::cursor::Cursor, Pos2, Vec2};
use rustc_hash::{FxHashMap, FxHashSet};
use taffy::{prelude::*, Overflow};
use winit::{
    dpi::PhysicalPosition,
    event::{ElementState, KeyEvent, Modifiers, MouseScrollDelta},
    window::CursorIcon,
};

use crate::{
    event_loop::DomContext,
    events::{self, DomEvent, EventState, LayoutEvent},
    renderer::{Renderer, ScreenDescriptor},
};

use super::tailwind::{StyleState, Tailwind};

pub struct Computed {
    /// The computed rect of the node, ready to be drawn
    pub rect: epaint::Rect,
    /// The computed galley of the text node, ready to be drawn
    pub galley: Option<Arc<epaint::Galley>>,
}

impl Default for Computed {
    fn default() -> Self {
        Self {
            rect: epaint::Rect::from_min_size(epaint::Pos2::ZERO, epaint::Vec2::ZERO),
            galley: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tag {
    View,
    Text,
}

pub struct NodeContext {
    pub tag: Tag,
    pub parent_id: Option<NodeId>,
    pub attrs: FxHashMap<Arc<str>, Arc<str>>,
    pub listeners: FxHashSet<Arc<str>>,
    pub styling: Tailwind,
    pub scroll: Vec2,
    pub computed: Computed,
}

impl NodeContext {
    pub fn get_text_cursor(&self, pick_position: Vec2) -> Option<Cursor> {
        if self.tag != Tag::Text {
            return None;
        }

        let galley = self.computed.galley.as_ref()?;
        let cursor = galley.cursor_from_pos(pick_position - self.computed.rect.min.to_vec2());
        Some(cursor)
    }
}

#[derive(Default, Clone, Copy, Debug)]
pub struct KeyboardState {
    pub modifiers: Modifiers,
}

#[derive(Default, Debug, Clone, Copy)]
pub struct CursorState {
    pub current_position: Pos2,
    pub drag_start_position: Option<Pos2>,
    pub drag_end_position: Option<Pos2>,
}

#[derive(Debug, Clone, Copy)]
pub struct SelectedNode {
    pub node_id: NodeId,
    pub parent_id: NodeId,
    /// The position of the node when it was selected
    pub computed_rect_when_selected: epaint::Rect,
    pub start_cursor: Cursor,
    pub end_cursor: Cursor,
}

#[derive(Debug, Clone, Copy)]
pub struct FocusedNode {
    pub node_id: NodeId,
    pub text_child_id: Option<NodeId>,
}

#[derive(Debug, Clone)]
pub struct DomState {
    pub window_position: PhysicalPosition<i32>,
    pub hovered: Vec<NodeId>,
    pub focused: Option<FocusedNode>,
    pub selection: Vec<SelectedNode>,
    pub keyboard_state: KeyboardState,
    pub cursor_state: CursorState,
    pub last_clicked: Option<(Instant, Option<NodeId>)>,
}

pub struct Dom {
    pub tree: TaffyTree<NodeContext>,
    templates: FxHashMap<String, Vec<NodeId>>,
    stack: Vec<NodeId>,
    pub element_id_mapping: FxHashMap<ElementId, NodeId>,
    common_tags_and_attr_keys: FxHashSet<Arc<str>>,
    pub state: DomState,
    context: DomContext,
}

impl Dom {
    pub fn new(context: DomContext) -> Dom {
        let mut tree = TaffyTree::<NodeContext>::new();

        let root_id = tree
            .new_leaf_with_context(
                Style::default(),
                NodeContext {
                    parent_id: None,
                    tag: Tag::View,
                    attrs: Default::default(),
                    styling: Tailwind::default(),
                    scroll: Default::default(),
                    computed: Default::default(),
                    listeners: Default::default(),
                },
            )
            .unwrap();

        let mut element_id_mapping = FxHashMap::default();
        element_id_mapping.insert(ElementId(0), root_id);

        let mut common_tags_and_attr_keys = FxHashSet::default();
        common_tags_and_attr_keys.insert("view".into());
        common_tags_and_attr_keys.insert("class".into());
        common_tags_and_attr_keys.insert("value".into());
        common_tags_and_attr_keys.insert("image".into());

        Dom {
            tree,
            templates: Default::default(),
            stack: Default::default(),
            element_id_mapping,
            common_tags_and_attr_keys,
            state: DomState {
                window_position: Default::default(),
                focused: None,
                hovered: vec![],
                selection: vec![],
                keyboard_state: Default::default(),
                cursor_state: Default::default(),
                last_clicked: None,
            },
            context,
        }
    }

    pub fn insert_node_before(&mut self, old_node_id: NodeId, new_id: NodeId) {
        let parent_id = self
            .tree
            .get_node_context_mut(old_node_id)
            .unwrap()
            .parent_id
            .unwrap();

        {
            let node = self.tree.get_node_context_mut(new_id).unwrap();
            node.parent_id = Some(parent_id);
        }

        // let parent = self.nodes.get_mut(parent_id).unwrap();
        let children = self.tree.children(parent_id).unwrap();
        let index = children
            .iter()
            .position(|child| *child == old_node_id)
            .unwrap();

        self.tree
            .insert_child_at_index(parent_id, index, new_id)
            .unwrap();
    }

    pub fn insert_node_after(&mut self, old_node_id: NodeId, new_id: NodeId) {
        let parent_id = self
            .tree
            .get_node_context_mut(old_node_id)
            .unwrap()
            .parent_id
            .unwrap();

        {
            let node = self.tree.get_node_context_mut(new_id).unwrap();
            node.parent_id = Some(parent_id);
        }

        let children = self.tree.children(parent_id).unwrap();
        let index = children
            .iter()
            .position(|child| *child == old_node_id)
            .unwrap();

        self.tree
            .insert_child_at_index(parent_id, index + 1, new_id)
            .unwrap();
    }

    fn load_path(&self, path: &[u8]) -> NodeId {
        let mut current_node_id = *self.stack.last().unwrap();

        for index in path {
            let new_id = self
                .tree
                .child_at_index(current_node_id, *index as usize)
                .unwrap();
            current_node_id = new_id;
        }

        current_node_id
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

    fn create_template_node(&mut self, node: &TemplateNode, parent_id: Option<NodeId>) -> NodeId {
        match *node {
            TemplateNode::Element {
                tag,
                attrs,
                children,
                ..
            } => {
                let mut node = NodeContext {
                    parent_id,
                    tag: if tag == "text" { Tag::Text } else { Tag::View },
                    attrs: attrs
                        .iter()
                        .filter_map(|val| {
                            if let TemplateAttribute::Static { name, value, .. } = val {
                                Some((self.get_tag_or_attr_key(name), (*value).into()))
                            } else {
                                None
                            }
                        })
                        .collect(),
                    styling: Tailwind::default(),
                    scroll: Vec2::ZERO,
                    computed: Default::default(),
                    listeners: Default::default(),
                };
                let style = self.get_initial_styling(&mut node);
                let node_id = self.tree.new_leaf_with_context(style, node).unwrap();

                for child in children {
                    let child = self.create_template_node(child, Some(node_id));

                    self.tree.add_child(node_id, child).unwrap();
                }

                node_id
            }
            TemplateNode::Text { text } => {
                let mut attrs = FxHashMap::default();
                attrs.insert(self.get_tag_or_attr_key("value"), text.into());
                attrs.insert(self.get_tag_or_attr_key("class"), "".into());

                let mut node = NodeContext {
                    parent_id,
                    tag: Tag::Text,
                    attrs,
                    styling: Tailwind::default(),
                    scroll: Vec2::ZERO,
                    computed: Default::default(),
                    listeners: Default::default(),
                };
                let style = self.get_initial_styling(&mut node);
                let node_id = self.tree.new_leaf_with_context(style, node).unwrap();

                node_id
            }

            TemplateNode::Dynamic { .. } => {
                let node_id = self
                    .tree
                    .new_leaf_with_context(
                        Style::default(),
                        NodeContext {
                            parent_id,
                            tag: Tag::View,
                            attrs: FxHashMap::default(),
                            styling: Tailwind::default(),
                            scroll: Vec2::ZERO,
                            computed: Default::default(),
                            listeners: Default::default(),
                        },
                    )
                    .unwrap();

                node_id
            }

            TemplateNode::DynamicText { .. } => {
                let mut attrs = FxHashMap::default();
                attrs.insert(self.get_tag_or_attr_key("class"), "".into());
                let node_id = self
                    .tree
                    .new_leaf_with_context(
                        Style::default(),
                        NodeContext {
                            parent_id,
                            tag: Tag::Text,
                            attrs,
                            styling: Tailwind::default(),
                            scroll: Vec2::ZERO,
                            computed: Default::default(),
                            listeners: Default::default(),
                        },
                    )
                    .unwrap();

                node_id
            }
        }
    }

    #[tracing::instrument(skip_all, name = "Dom::apply_mutations")]
    pub fn apply_mutations(&mut self, mutations: Mutations) {
        for template in mutations.templates {
            let mut children = Vec::with_capacity(template.roots.len());
            for root in template.roots {
                let id: NodeId =
                    self.create_template_node(root, Some(self.element_id_mapping[&ElementId(0)]));
                children.push(id);
            }
            self.templates.insert(template.name.to_string(), children);
        }

        for edit in mutations.edits {
            match edit {
                dioxus::core::Mutation::LoadTemplate { name, index, id } => {
                    let template_id = self.templates[name][index];
                    let new_id =
                        self.clone_node(template_id, self.element_id_mapping[&ElementId(0)]);
                    self.stack.push(new_id);
                    self.element_id_mapping.insert(id, new_id);
                }
                dioxus::core::Mutation::AssignId { path, id } => {
                    let node_id = self.load_path(path);
                    self.element_id_mapping.insert(id, node_id);
                }

                dioxus::core::Mutation::CreatePlaceholder { id } => {
                    let node = NodeContext {
                        parent_id: None,
                        attrs: FxHashMap::default(),
                        computed: Default::default(),
                        listeners: Default::default(),
                        scroll: Vec2::ZERO,
                        styling: Tailwind::default(),
                        tag: Tag::View,
                    };

                    let node_id = self
                        .tree
                        .new_leaf_with_context(Style::default(), node)
                        .unwrap();

                    self.element_id_mapping.insert(id, node_id);
                    self.stack.push(node_id);
                }

                dioxus::core::Mutation::AppendChildren { m, id } => {
                    let children = self.stack.split_off(self.stack.len() - m);
                    let parent = self.element_id_mapping[&id];
                    for child in children {
                        self.tree.add_child(parent, child).unwrap();
                    }
                }
                dioxus::core::Mutation::NewEventListener { name, id } => {
                    let name = self.get_tag_or_attr_key(name);
                    let node_id = self.element_id_mapping[&id];
                    let node = self.tree.get_node_context_mut(node_id).unwrap();
                    node.listeners.insert(name);
                }
                dioxus::core::Mutation::RemoveEventListener { name, id } => {
                    let name = self.get_tag_or_attr_key(name);
                    let node_id = self.element_id_mapping[&id];
                    let node = self.tree.get_node_context_mut(node_id).unwrap();
                    node.listeners.remove(&name);
                }
                dioxus::core::Mutation::SetAttribute {
                    name, value, id, ..
                } => {
                    let node_id = self.element_id_mapping[&id];
                    if let BorrowedAttributeValue::None = &value {
                        let node = self.tree.get_node_context_mut(node_id).unwrap();
                        node.attrs.remove(name);
                    } else {
                        let key = self.get_tag_or_attr_key(name);
                        let node = self.tree.get_node_context_mut(node_id).unwrap();
                        node.attrs.insert(
                            key,
                            match value {
                                BorrowedAttributeValue::Int(val) => (val.to_string()).into(),
                                BorrowedAttributeValue::Bool(val) => (val.to_string()).into(),
                                BorrowedAttributeValue::Float(val) => (val.to_string()).into(),
                                BorrowedAttributeValue::Text(val) => val.into(),
                                BorrowedAttributeValue::None => "".into(),
                                BorrowedAttributeValue::Any(_) => unimplemented!(),
                            },
                        );
                    }
                }
                dioxus::core::Mutation::CreateTextNode { value, id } => {
                    let mut attrs = FxHashMap::default();
                    attrs.insert(self.get_tag_or_attr_key("value"), value.into());
                    attrs.insert(self.get_tag_or_attr_key("class"), "".into());

                    let node = NodeContext {
                        parent_id: None,
                        attrs,
                        computed: Default::default(),
                        listeners: Default::default(),
                        scroll: Vec2::ZERO,
                        styling: Tailwind::default(),
                        tag: Tag::Text,
                    };
                    let node_id = self
                        .tree
                        .new_leaf_with_context(Style::default(), node)
                        .unwrap();

                    self.element_id_mapping.insert(id, node_id);
                    self.stack.push(node_id);
                }
                dioxus::core::Mutation::HydrateText { path, value, id } => {
                    let node_id = self.load_path(path);
                    let key = self.get_tag_or_attr_key("value");
                    self.element_id_mapping.insert(id, node_id);
                    let node = self.tree.get_node_context_mut(node_id).unwrap();
                    node.attrs.insert(key, value.into());
                }
                dioxus::core::Mutation::SetText { value, id } => {
                    let node_id = self.element_id_mapping[&id];
                    let key = self.get_tag_or_attr_key("value");
                    let node = self.tree.get_node_context_mut(node_id).unwrap();
                    node.attrs.insert(key, value.into());
                    self.tree.mark_dirty(node_id).unwrap();
                    self.state
                        .selection
                        .retain(|range| range.node_id != node_id);
                }
                dioxus::core::Mutation::ReplaceWith { id, m } => {
                    let new_nodes = self.stack.split_off(self.stack.len() - m);
                    let old_node_id = self.element_id_mapping[&id];
                    for new_id in new_nodes {
                        self.insert_node_before(old_node_id, new_id);
                    }
                    self.remove_node(old_node_id);
                }
                dioxus::core::Mutation::ReplacePlaceholder { path, m } => {
                    let new_nodes = self.stack.split_off(self.stack.len() - m);
                    let old_node_id = self.load_path(path);

                    for new_id in new_nodes {
                        self.insert_node_before(old_node_id, new_id);
                    }

                    self.remove_node(old_node_id);
                }

                dioxus::core::Mutation::InsertAfter { id, m } => {
                    let new_nodes = self.stack.split_off(self.stack.len() - m);
                    let old_node_id = self.element_id_mapping[&id];
                    for new_id in new_nodes.into_iter().rev() {
                        self.insert_node_after(old_node_id, new_id);
                    }
                }

                dioxus::core::Mutation::InsertBefore { id, m } => {
                    let new_nodes = self.stack.split_off(self.stack.len() - m);
                    let old_node_id = self.element_id_mapping[&id];
                    for new_id in new_nodes {
                        self.insert_node_before(old_node_id, new_id);
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

        self.check_and_set_cursor_icon();
    }

    /// Clone node and its children, they all get new ids
    pub fn clone_node(&mut self, node_id: NodeId, parent_id: NodeId) -> NodeId {
        let (tag, attrs, styling) = {
            let ctx = self.tree.get_node_context_mut(node_id).unwrap();

            (ctx.tag.clone(), ctx.attrs.clone(), ctx.styling.clone())
        };

        let mut node = NodeContext {
            parent_id: Some(parent_id),
            tag,
            attrs,
            styling,
            scroll: Vec2::ZERO,
            computed: Default::default(),
            listeners: Default::default(),
        };
        let style = self.get_initial_styling(&mut node);

        let cloned_node = self.tree.new_leaf_with_context(style, node).unwrap();

        for children in self.tree.children(node_id).unwrap().iter() {
            let new_id = self.clone_node(*children, cloned_node);
            self.tree.add_child(cloned_node, new_id).unwrap();
        }

        cloned_node
    }

    pub fn get_root_id(&self) -> NodeId {
        self.element_id_mapping[&ElementId(0)]
    }

    pub fn remove_node(&mut self, id: NodeId) {
        // remove children recursively
        for child in self.tree.children(id).unwrap().iter() {
            self.remove_node(*child);
        }
        self.tree.remove(id).unwrap();
    }

    pub fn print_tree(&mut self) {
        self.tree.print_tree(self.get_root_id());
    }

    pub fn get_initial_styling(&mut self, node_context: &mut NodeContext) -> Style {
        let Some(class) = node_context.attrs.get(&self.get_tag_or_attr_key("class")) else {
            return Style::default();
        };
        node_context
            .styling
            .get_style(class, &StyleState::default())
    }

    /// Return true to continue traversal, false to stop
    pub fn traverse_tree(
        &mut self,
        id: NodeId,
        callback: &mut impl FnMut(&mut Dom, NodeId) -> bool,
    ) {
        let should_continue = callback(self, id);
        if !should_continue {
            return;
        }
        for child in self.tree.children(id).unwrap().iter() {
            self.traverse_tree(*child, callback);
        }
    }

    pub fn traverse_tree_with_parent(
        &mut self,
        id: NodeId,
        parent_id: Option<NodeId>,
        callback: &mut impl FnMut(&mut Dom, NodeId, Option<NodeId>) -> bool,
    ) {
        if let Some(parent_id) = parent_id {
            let should_continue = callback(self, id, Some(parent_id));
            if !should_continue {
                return;
            }
        } else {
            let should_continue = callback(self, id, None);
            if !should_continue {
                return;
            }
        };

        for child in self.tree.children(id).unwrap().iter() {
            self.traverse_tree_with_parent(*child, Some(id), callback);
        }
    }

    pub fn traverse_tree_mut_with_parent_and_data<T>(
        &mut self,
        id: NodeId,
        parent_id: Option<NodeId>,
        data: &T,
        callback: &mut impl FnMut(&mut Dom, NodeId, Option<NodeId>, &T) -> (bool, T),
    ) {
        let data = if let Some(parent_id) = parent_id {
            let (should_continue, new_data) = callback(self, id, Some(parent_id), data);
            if !should_continue {
                return;
            }

            new_data
        } else {
            let (should_continue, new_data) = callback(self, id, None, data);
            if !should_continue {
                return;
            }

            new_data
        };

        for child in self.tree.children(id).unwrap().iter() {
            self.traverse_tree_mut_with_parent_and_data(*child, Some(id), &data, callback);
        }
    }

    fn send_event_to_element(
        &mut self,
        node_id: NodeId,
        listener: &str,
        event: Arc<events::Event>,
        bubbles: bool,
    ) {
        let listener = self.get_tag_or_attr_key(listener);
        let mut current_node_id = node_id;
        if bubbles {
            loop {
                let Some(node) = self.tree.get_node_context(current_node_id) else {
                    // can happen if the tree isn't fully built yet
                    break;
                };
                let Some(name) = node.listeners.get(&listener) else {
                    // bubble up if there are no listeners at all
                    if let Some(parent_id) = node.parent_id {
                        current_node_id = parent_id;
                        continue;
                    } else {
                        break;
                    }
                };

                let Some((element_id, ..)) = self
                    .element_id_mapping
                    .iter()
                    .find(|(_, id)| **id == current_node_id)
                else {
                    return;
                };

                self.context
                    .event_sender
                    .send(DomEvent {
                        name: name.clone(),
                        data: event.clone(),
                        element_id: *element_id,
                        bubbles: false,
                    })
                    .unwrap();
                break;
            }
        } else {
            let Some(node) = self.tree.get_node_context(current_node_id) else {
                // can happen if the tree isn't fully built yet
                return;
            };
            let Some(name) = node.listeners.get(&listener) else {
                return;
            };

            let Some((element_id, ..)) = self
                .element_id_mapping
                .iter()
                .find(|(_, id)| **id == current_node_id)
            else {
                return;
            };

            self.context
                .event_sender
                .send(DomEvent {
                    name: name.clone(),
                    data: event.clone(),
                    element_id: *element_id,
                    bubbles: false,
                })
                .unwrap();
        }
    }

    fn translate_mouse_pos(
        pos_in_pixels: &PhysicalPosition<f64>,
        screen_descriptor: &ScreenDescriptor,
    ) -> epaint::Pos2 {
        epaint::pos2(
            pos_in_pixels.x as f32 / screen_descriptor.pixels_per_point,
            pos_in_pixels.y as f32 / screen_descriptor.pixels_per_point,
        )
    }

    /// Sets the hovered, focused and currently selected nodes
    pub fn on_mouse_move(
        &mut self,
        position: &PhysicalPosition<f64>,
        screen_descriptor: &ScreenDescriptor,
    ) -> bool {
        let position = Self::translate_mouse_pos(position, screen_descriptor);
        self.state.cursor_state.current_position = position;
        self.state.hovered.clear();
        self.traverse_tree(self.get_root_id(), &mut |dom, id| {
            let node = dom.tree.get_node_context_mut(id).unwrap();
            let rect = node.computed.rect;
            let is_hovered = rect.contains(epaint::Pos2::new(
                dom.state.cursor_state.current_position.x as f32,
                dom.state.cursor_state.current_position.y as f32,
            ));
            if is_hovered {
                dom.state.hovered.push(id);
            }
            true
        });

        if self.state.cursor_state.drag_start_position.is_some()
            && self.state.cursor_state.drag_end_position.is_none()
        {
            if let Some(start_position) = self.state.cursor_state.drag_start_position {
                let min = Pos2::new(
                    start_position
                        .x
                        .min(self.state.cursor_state.current_position.x),
                    start_position
                        .y
                        .min(self.state.cursor_state.current_position.y),
                );

                let max = Pos2::new(
                    start_position
                        .x
                        .max(self.state.cursor_state.current_position.x),
                    start_position
                        .y
                        .max(self.state.cursor_state.current_position.y),
                );

                let selection_rect = epaint::Rect::from_min_max(min, max);

                self.state.selection.clear();
                self.traverse_tree(self.get_root_id(), &mut |dom, id| {
                    let node = dom.tree.get_node_context_mut(id).unwrap();
                    if let Some(selection_mode) = node.attrs.get("global_selection_mode") {
                        if *selection_mode == "off".into() {
                            return false;
                        }
                    }
                    if node.tag != Tag::Text {
                        return true;
                    }
                    if node.computed.rect.intersects(selection_rect) {
                        let mut start_cursor =
                            node.get_text_cursor(start_position.to_vec2()).unwrap();
                        let mut end_cursor = node
                            .get_text_cursor(
                                dom.state
                                    .cursor_state
                                    .drag_end_position
                                    .unwrap_or(dom.state.cursor_state.current_position)
                                    .to_vec2(),
                            )
                            .unwrap();

                        // swap cursors if the selection is backwards
                        if start_cursor.pcursor.offset > end_cursor.pcursor.offset {
                            std::mem::swap(&mut start_cursor, &mut end_cursor);
                        }

                        dom.set_selection(id, start_cursor, end_cursor, false);
                    }

                    false
                });
            }

            // send drag event to the focused node
            if let Some(focused) = self.state.focused {
                self.send_event_to_element(
                    focused.node_id,
                    "drag",
                    Arc::new(events::Event::Drag(events::DragEvent {
                        state: EventState::new(self, focused.node_id),
                    })),
                    true,
                );
            }
        }

        self.check_and_set_cursor_icon();

        true
    }

    pub fn on_mouse_input(
        &mut self,
        _renderer: &Renderer,
        button: &winit::event::MouseButton,
        state: &winit::event::ElementState,
    ) -> bool {
        if button == &winit::event::MouseButton::Left
            && state == &winit::event::ElementState::Pressed
        {
            self.state.cursor_state.drag_start_position =
                Some(self.state.cursor_state.current_position);
            self.state.cursor_state.drag_end_position = None;
        } else if button == &winit::event::MouseButton::Left
            && state == &winit::event::ElementState::Released
        {
            self.state.cursor_state.drag_end_position =
                Some(self.state.cursor_state.current_position);
        }

        // find first element with tabindex
        let mut focused_text_child = None;
        let focused_node = self.state.hovered.clone().iter().rev().find_map(|id| {
            let Some(node) = self.tree.get_node_context(*id) else {
                return None;
            };

            if node.tag == Tag::Text {
                focused_text_child = Some(*id);
            }

            if node.attrs.get("tabindex").is_some() || node.listeners.contains("click") {
                let node = FocusedNode {
                    node_id: *id,
                    text_child_id: focused_text_child,
                };

                self.send_event_to_element(
                    *id,
                    "focus",
                    Arc::new(events::Event::Focus(events::FocusEvent {
                        state: EventState::new(self, *id),
                    })),
                    true,
                );

                Some(node)
            } else {
                None
            }
        });
        self.set_focus(focused_node);

        if let Some(focused) = self.state.focused {
            let text_cursor_position = if let Some(text_child_id) = focused.text_child_id {
                let node = self.tree.get_node_context(text_child_id).unwrap();
                let cursor = node
                    .get_text_cursor(self.state.cursor_state.current_position.to_vec2())
                    .unwrap();

                Some(cursor.pcursor.offset)
            } else {
                None
            };

            let pressed_data = Arc::new(events::Event::Click(events::ClickEvent {
                state: EventState::new(self, focused.node_id),
                button: button.clone(),
                element_state: ElementState::Pressed,
                text_cursor_position,
            }));

            let not_pressed_data = Arc::new(events::Event::Click(events::ClickEvent {
                state: EventState::new(self, focused.node_id),
                button: button.clone(),
                element_state: ElementState::Released,
                text_cursor_position,
            }));

            match state {
                winit::event::ElementState::Pressed => {
                    self.send_event_to_element(
                        focused.node_id,
                        "click",
                        pressed_data.clone(),
                        true,
                    );
                    self.send_event_to_element(
                        focused.node_id,
                        "mousedown",
                        pressed_data.clone(),
                        true,
                    );
                }
                winit::event::ElementState::Released => {
                    self.send_event_to_element(
                        focused.node_id,
                        "mouseup",
                        not_pressed_data.clone(),
                        true,
                    );
                }
            }
        }

        // if we clicked on the same node as last time, then we should select the word
        if let winit::event::ElementState::Pressed = state {
            self.state.selection.clear();

            let selected_something =
                if let Some((time_last_clicked, last_clicked)) = self.state.last_clicked {
                    if Instant::now() - time_last_clicked > std::time::Duration::from_millis(500) {
                        false
                    } else if last_clicked.is_some() {
                        focused_text_child == last_clicked
                    } else {
                        false
                    }
                } else {
                    false
                };

            if selected_something {
                let node = self
                    .tree
                    .get_node_context(focused_text_child.unwrap())
                    .unwrap();

                let galley = node.computed.galley.as_ref().unwrap();
                let cursor = node
                    .get_text_cursor(self.state.cursor_state.current_position.to_vec2())
                    .unwrap();

                let mut start_cursor = cursor;
                let mut end_cursor = cursor;

                // find the start of the word
                while start_cursor.pcursor.offset > 0 {
                    let prev_char = galley.text().chars().nth(start_cursor.pcursor.offset - 1);
                    if prev_char.is_none() {
                        break;
                    }
                    let prev_char = prev_char.unwrap();

                    if prev_char.is_whitespace() {
                        break;
                    }

                    start_cursor.pcursor.offset -= 1;
                }

                // find the end of the word
                while end_cursor.pcursor.offset < galley.text().len() {
                    let next_char = galley.text().chars().nth(end_cursor.pcursor.offset);
                    if next_char.is_none() {
                        break;
                    }
                    let next_char = next_char.unwrap();

                    if next_char.is_whitespace() {
                        break;
                    }

                    end_cursor.pcursor.offset += 1;
                }

                start_cursor.ccursor.index = start_cursor.pcursor.offset;
                end_cursor.ccursor.index = end_cursor.pcursor.offset;

                start_cursor.rcursor.column = start_cursor.pcursor.offset;
                end_cursor.rcursor.column = end_cursor.pcursor.offset;

                // swap cursors if the selection is backwards
                if start_cursor.pcursor.offset > end_cursor.pcursor.offset {
                    std::mem::swap(&mut start_cursor, &mut end_cursor);
                }

                self.set_selection(focused_text_child.unwrap(), start_cursor, end_cursor, true);
            }

            self.state.last_clicked = if selected_something {
                None
            } else {
                Some((Instant::now(), focused_text_child))
            }
        }

        true
    }

    /// Scrolls the last node that is scrollable
    pub fn on_scroll(&mut self, delta: &MouseScrollDelta) -> bool {
        let Some(scroll_node) = self.state.hovered.iter().rev().find_map(|id| {
            let style = self.tree.style(*id).unwrap();

            if style.overflow.x != Overflow::Scroll && style.overflow.y != Overflow::Scroll {
                return None;
            }

            Some(id)
        }) else {
            return false;
        };

        let tick_size = 30.0;
        let mut scroll = Vec2::ZERO;
        match delta {
            MouseScrollDelta::LineDelta(_x, y) => {
                if self.state.keyboard_state.modifiers.state().shift_key() {
                    scroll.x -= y * tick_size;
                } else {
                    scroll.y -= y * tick_size;
                }
            }
            MouseScrollDelta::PixelDelta(pos) => {
                scroll += Vec2::new(pos.x as f32, pos.y as f32);
            }
        }

        let (total_scroll_width, total_scroll_height) = {
            let layout = self.tree.layout(*scroll_node).unwrap();

            (layout.scroll_width(), layout.scroll_height())
        };

        let node = self.tree.get_node_context_mut(*scroll_node).unwrap();
        scroll += node.scroll;
        node.scroll.x = scroll.x.max(0.0).min(total_scroll_width);
        node.scroll.y = scroll.y.max(0.0).min(total_scroll_height);

        true
    }

    pub fn on_keyboard_input(&mut self, input: &KeyEvent) -> bool {
        let Some(focused) = self.state.focused else {
            return false;
        };

        if input.state.is_pressed() {
            self.send_event_to_element(
                focused.node_id,
                "input",
                Arc::new(events::Event::Input(events::InputEvent {
                    state: EventState::new(self, focused.node_id),
                    logical_key: input.logical_key.clone(),
                    physical_key: input.physical_key,
                    text: input.text.clone(),
                })),
                true,
            );
        }

        self.send_event_to_element(
            focused.node_id,
            match input.state {
                winit::event::ElementState::Pressed => "keydown",
                winit::event::ElementState::Released => "keyup",
            },
            Arc::new(events::Event::Key(events::KeyInput {
                state: EventState::new(self, focused.node_id),
                element_state: input.state,
                logical_key: input.logical_key.clone(),
                physical_key: input.physical_key,
                text: input.text.clone(),
            })),
            true,
        );

        if let Some(text_child_id) = focused.text_child_id {
            if let winit::keyboard::Key::Character(c) = &input.logical_key {
                // check if we need to select all
                if *c == "a" && self.state.modifiers().state().control_key() {
                    let node = self.tree.get_node_context(text_child_id).unwrap();
                    let galley = node.computed.galley.as_ref().unwrap();

                    // select all of the text
                    let start = galley.cursor_from_pos(Vec2::ZERO);
                    let end = galley.end();
                    self.set_selection(text_child_id, start, end, true);
                }
            }

            // if self.state.modifiers().state().shift_key() {
            //     let parent = self.tree.get_node_context(focused.node_id).unwrap();
            //     let node = self.tree.get_node_context(text_child_id).unwrap();
            //     let galley = node.computed.galley.as_ref().unwrap();

            //     // if not selected anything, we use the cursor, otherwise we nothing
            //     let (start_cursor, end_cursor) = if self.state.selection.is_empty() {
            //         let cursor = parent
            //             .attrs
            //             .get("text_cursor")
            //             .unwrap()
            //             .parse::<usize>()
            //             .unwrap();
            //         let mut m = Cursor::default();
            //         m.ccursor.index = cursor;
            //         m.pcursor.offset = cursor;
            //         m.rcursor.column = cursor;
            //         (m, m)
            //     } else {
            //         let select = self.state.selection.first().unwrap();
            //         (select.start_cursor, select.end_cursor)
            //     };

            //     if let winit::keyboard::Key::Named(named) = &input.logical_key {
            //         match named {
            //             winit::keyboard::NamedKey::ArrowLeft => {
            //                 // we select 1 to the left!

            //                 if start_cursor == end
            //             }
            //             _ => {}
            //         }
            //     }
            // }

            // check if we need to select anything with shift
            // if
        }

        true
    }

    /// sends an event to the element that the layout has changed
    pub fn on_layout_changed(&mut self, nodes: &[NodeId]) {
        for node_id in nodes {
            let rect = self.tree.get_node_context(*node_id).unwrap().computed.rect;
            self.send_event_to_element(
                *node_id,
                "layout",
                Arc::new(events::Event::Layout(LayoutEvent {
                    state: EventState::new(self, *node_id),
                    rect,
                })),
                false,
            );
        }
    }

    pub fn on_window_resize(&mut self) {
        // send all nodes a layout event
        self.traverse_tree(self.get_root_id(), &mut |dom, id| {
            let rect = dom.tree.get_node_context(id).unwrap().computed.rect;
            dom.send_event_to_element(
                id,
                "layout",
                Arc::new(events::Event::Layout(LayoutEvent {
                    state: EventState::new(dom, id),
                    rect,
                })),
                false,
            );
            true
        });
    }

    pub fn on_window_moved(&mut self, position: &PhysicalPosition<i32>) {
        self.state.window_position = *position;
    }

    pub fn get_event_state(&mut self, node_id: NodeId) -> EventState {
        EventState::new(self, node_id)
    }

    pub fn set_selection(
        &mut self,
        node_id: NodeId,
        start_cursor: Cursor,
        end_cursor: Cursor,
        clear: bool,
    ) {
        let node = self.tree.get_node_context(node_id).unwrap();

        if node.tag != Tag::Text {
            log::warn!("set_selection called on non-text node");
            return;
        }

        if clear {
            self.state.selection.clear();
        }
        self.state.selection.push(SelectedNode {
            node_id,
            parent_id: node.parent_id.unwrap(),
            computed_rect_when_selected: node.computed.rect,
            start_cursor,
            end_cursor,
        });

        self.send_event_to_element(
            node.parent_id.unwrap(),
            "select",
            Arc::new(events::Event::Select(events::SelectEvent {
                state: EventState::new(self, node_id),
                start_cursor,
                end_cursor,
            })),
            true,
        );
    }

    pub fn set_focus(&mut self, focused_node: Option<FocusedNode>) {
        let prev_focused = self.state.focused;
        self.state.focused = focused_node;

        if let Some(prev_focused) = prev_focused {
            if let Some(focused) = self.state.focused {
                if focused.node_id == prev_focused.node_id {
                    return;
                }
            }

            self.send_event_to_element(
                prev_focused.node_id,
                "blur",
                Arc::new(events::Event::Blur(events::BlurEvent {
                    state: EventState::new(self, prev_focused.node_id),
                })),
                true,
            );
        }
    }

    pub fn check_and_set_cursor_icon(&mut self) {
        let mut new_cursor_icon = CursorIcon::Default;

        // check if we're hovering over a node with tabindex or click listener
        if let Some(hovered) = self.state.hovered.last() {
            let node = self.tree.get_node_context(*hovered).unwrap();
            let node = if node.tag == Tag::Text {
                self.tree.get_node_context(node.parent_id.unwrap()).unwrap()
            } else {
                node
            };

            if node.attrs.get("tabindex").is_some() || node.listeners.contains("click") {
                new_cursor_icon = CursorIcon::Pointer;
            }
        }

        // if we're just hovering over a text node, then we should set the cursor to text
        if new_cursor_icon == CursorIcon::Default {
            if let Some(hovered) = self.state.hovered.last() {
                let node = self.tree.get_node_context(*hovered).unwrap();
                if node.tag == Tag::Text {
                    new_cursor_icon = CursorIcon::Text;
                }
            }
        }

        // if node itself has a class put on it, then we should set the cursor to that as highest priority
        if let Some(hovered) = self.state.hovered.last() {
            let node = self.tree.get_node_context(*hovered).unwrap();
            let node = if node.tag == Tag::Text {
                self.tree.get_node_context(node.parent_id.unwrap()).unwrap()
            } else {
                node
            };
            let classes = node.attrs.get("class");
            for class in classes.unwrap_or(&"".into()).split_whitespace() {
                let class = class.strip_prefix("cursor-");
                if let Some(class) = class {
                    match class {
                        "default" => new_cursor_icon = CursorIcon::Default,
                        "pointer" => new_cursor_icon = CursorIcon::Pointer,
                        "wait" => new_cursor_icon = CursorIcon::Wait,
                        "text" => new_cursor_icon = CursorIcon::Text,
                        "move" => new_cursor_icon = CursorIcon::Move,
                        "help" => new_cursor_icon = CursorIcon::Help,
                        "not-allowed" => new_cursor_icon = CursorIcon::NotAllowed,
                        "context-menu" => new_cursor_icon = CursorIcon::ContextMenu,
                        "progress" => new_cursor_icon = CursorIcon::Progress,
                        "cell" => new_cursor_icon = CursorIcon::Cell,
                        "crosshair" => new_cursor_icon = CursorIcon::Crosshair,
                        "vertical-text" => new_cursor_icon = CursorIcon::VerticalText,
                        "alias" => new_cursor_icon = CursorIcon::Alias,
                        "copy" => new_cursor_icon = CursorIcon::Copy,
                        "no-drop" => new_cursor_icon = CursorIcon::NoDrop,
                        "grab" => new_cursor_icon = CursorIcon::Grab,
                        "grabbing" => new_cursor_icon = CursorIcon::Grabbing,
                        "all-scroll" => new_cursor_icon = CursorIcon::AllScroll,
                        "col-resize" => new_cursor_icon = CursorIcon::ColResize,
                        "row-resize" => new_cursor_icon = CursorIcon::RowResize,
                        "n-resize" => new_cursor_icon = CursorIcon::NResize,
                        "e-resize" => new_cursor_icon = CursorIcon::EResize,
                        "s-resize" => new_cursor_icon = CursorIcon::SResize,
                        "w-resize" => new_cursor_icon = CursorIcon::WResize,
                        "ne-resize" => new_cursor_icon = CursorIcon::NeResize,
                        "nw-resize" => new_cursor_icon = CursorIcon::NwResize,
                        "se-resize" => new_cursor_icon = CursorIcon::SeResize,
                        "sw-resize" => new_cursor_icon = CursorIcon::SwResize,
                        "ew-resize" => new_cursor_icon = CursorIcon::EwResize,
                        "ns-resize" => new_cursor_icon = CursorIcon::NsResize,
                        "nesw-resize" => new_cursor_icon = CursorIcon::NeswResize,
                        "nwse-resize" => new_cursor_icon = CursorIcon::NwseResize,
                        "zoom-in" => new_cursor_icon = CursorIcon::ZoomIn,
                        "zoom-out" => new_cursor_icon = CursorIcon::ZoomOut,
                        _ => {}
                    }
                }
            }
        }

        if self.context.current_cursor_icon != new_cursor_icon {
            self.context.window.set_cursor_icon(new_cursor_icon);
            self.context.current_cursor_icon = new_cursor_icon;
        }
    }
}
