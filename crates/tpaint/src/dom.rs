use std::sync::Arc;

use dioxus::{
    core::{BorrowedAttributeValue, ElementId, Mutations},
    prelude::{TemplateAttribute, TemplateNode},
};
use epaint::{Pos2, Vec2};
use rustc_hash::{FxHashMap, FxHashSet};
use taffy::{prelude::*, Overflow};
use tokio::sync::mpsc::UnboundedSender;
use winit::{dpi::PhysicalPosition, event::MouseScrollDelta};

use crate::{
    events::{self, DomEvent},
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

pub struct NodeContext {
    pub tag: Arc<str>,
    pub parent_id: Option<NodeId>,
    pub attrs: FxHashMap<Arc<str>, Arc<str>>,
    pub styling: Tailwind,
    pub scroll: Vec2,
    pub computed: Computed,
}

#[derive(Default, Clone, Copy, Debug)]
pub struct KeyboardState {
    pub modifiers: winit::event::ModifiersState,
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
    pub parent_id: Option<NodeId>,
    /// The position of the node when it was selected
    pub computed_rect_when_selected: epaint::Rect,
}

#[derive(Debug, Clone, Copy)]
pub struct FocusedNode {
    pub node_id: NodeId,
    pub text_cursor: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct DomState {
    pub hovered: Vec<NodeId>,
    pub focused: Option<FocusedNode>,
    pub selection: Vec<SelectedNode>,
    pub keyboard_state: KeyboardState,
    pub cursor_state: CursorState,
}

pub struct Dom {
    pub tree: TaffyTree<NodeContext>,
    templates: FxHashMap<String, Vec<NodeId>>,
    stack: Vec<NodeId>,
    pub element_id_mapping: FxHashMap<ElementId, NodeId>,
    common_tags_and_attr_keys: FxHashSet<Arc<str>>,
    pub event_listeners: FxHashMap<ElementId, Vec<Arc<str>>>,
    pub state: DomState,
    event_sender: UnboundedSender<DomEvent>,
}

impl Dom {
    pub fn new(event_sender: UnboundedSender<DomEvent>) -> Dom {
        let mut tree = TaffyTree::<NodeContext>::new();

        let root_id = tree
            .new_leaf_with_context(
                Style::default(),
                NodeContext {
                    parent_id: None,
                    tag: "view".into(),
                    attrs: Default::default(),
                    styling: Tailwind::default(),
                    scroll: Default::default(),
                    computed: Default::default(),
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
            event_listeners: Default::default(),
            state: DomState {
                focused: None,
                hovered: vec![],
                selection: vec![],
                keyboard_state: Default::default(),
                cursor_state: Default::default(),
            },
            event_sender,
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
                    tag: self.get_tag_or_attr_key(tag),
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
                attrs.insert(self.get_tag_or_attr_key("class"), "w-full".into());
                attrs.insert(self.get_tag_or_attr_key("value"), text.into());
                let mut node = NodeContext {
                    parent_id,
                    tag: "text".into(),
                    attrs,
                    styling: Tailwind::default(),
                    scroll: Vec2::ZERO,
                    computed: Default::default(),
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
                            tag: "view".into(),
                            attrs: FxHashMap::default(),
                            styling: Tailwind::default(),
                            scroll: Vec2::ZERO,
                            computed: Default::default(),
                        },
                    )
                    .unwrap();

                node_id
            }

            TemplateNode::DynamicText { .. } => {
                let mut attrs = FxHashMap::default();
                attrs.insert(self.get_tag_or_attr_key("class"), "w-full".into());

                let node_id = self
                    .tree
                    .new_leaf_with_context(
                        Style::default(),
                        NodeContext {
                            parent_id,
                            tag: "text".into(),
                            attrs,
                            styling: Tailwind::default(),
                            scroll: Vec2::ZERO,
                            computed: Default::default(),
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
            println!("inserting template {:?}", template.name);
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

                        scroll: Vec2::ZERO,
                        styling: Tailwind::default(),
                        tag: "placeholder".into(),
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
                    // let node_id = self.element_id_mapping[&id];
                    if let Some(listeners) = self.event_listeners.get_mut(&id) {
                        listeners.push(name);
                    } else {
                        self.event_listeners.insert(id, vec![name]);
                    }
                }
                dioxus::core::Mutation::RemoveEventListener { name, id } => {
                    let name = self.get_tag_or_attr_key(name);
                    if let Some(listeners) = self.event_listeners.get_mut(&id) {
                        listeners.retain(|val| val != &name);
                    }
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
                    attrs.insert(self.get_tag_or_attr_key("class"), "w-full".into());

                    let node = NodeContext {
                        parent_id: None,
                        attrs,
                        computed: Default::default(),
                        scroll: Vec2::ZERO,
                        styling: Tailwind::default(),
                        tag: "text".into(),
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

    fn send_event_to_element(&self, node_id: NodeId, listener: &str, event: Arc<events::Event>) {
        let Some((element_id, ..)) = self
            .element_id_mapping
            .iter()
            .find(|(_, id)| **id == node_id)
        else {
            return;
        };
        let Some(listeners) = self.event_listeners.get(&element_id) else {
            return;
        };

        let Some(name) = listeners.iter().find(|name| (name as &str) == listener) else {
            return;
        };

        self.event_sender
            .send(DomEvent {
                name: name.clone(),
                data: event.clone(),
                element_id: *element_id,
                bubbles: true,
            })
            .unwrap();
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
            let is_hovered = rect.contains(epaint::Pos2::new(position.x as f32, position.y as f32));
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
                    if node.tag != "text".into() {
                        return true;
                    }
                    if node.computed.rect.intersects(selection_rect) {
                        dom.state.selection.push(SelectedNode {
                            node_id: id,
                            parent_id: node.parent_id,
                            computed_rect_when_selected: node.computed.rect,
                        });
                    }

                    false
                });
            }

            self.handle_manual_selection();
        }

        true
    }

    pub fn handle_manual_selection(&mut self) {
        // on input fields you want to select the text on the mouse position, but since the text is not as big as the parent container we need to check this.
        // let mut only_parent_of_text_clicked = None;

        // self.traverse_tree_with_parent(self.get_root_id(), None, &mut |dom, node_id, parent_id| {
        //     // if !specific_nodes.is_empty() && !specific_nodes.contains(&node.id) {
        //     //     return true;
        //     // }
        //     let [node, parent] = dom
        //         .tree
        //         .get_disjoint_node_context_mut([node_id, parent_id.unwrap()])
        //         .unwrap();

        //     if node.tag == "text".into() {
        //         only_parent_of_text_clicked = None;
        //         let relative_position = dom.state.cursor_state.current_position.to_vec2()
        //             - node.computed.rect.min.to_vec2();
        //         let text = node.attrs.get("value").unwrap();
        //         let galley = node.styling.get_font_galley(
        //             text,
        //             &self.renderer.taffy,
        //             &self.renderer.fonts,
        //             &parent.unwrap().styling,
        //         );
        //         let cursor = galley.cursor_from_pos(relative_position);
        //         self.cursor_state.cursor = cursor;
        //         return false;
        //     }

        //     if node.attrs.get("cursor").is_some() {
        //         only_parent_of_text_clicked = Some(node_id);

        //         return true;
        //     }

        //     true
        // });

        // if let Some(parent_id) = only_parent_of_text_clicked {
        //     let parent = vdom.nodes.get(parent_id).unwrap();
        //     let node = vdom.nodes.get(*parent.children.first().unwrap()).unwrap();

        //     let relative_position = mouse_pos.to_vec2() - node.computed.rect.min.to_vec2();
        //     let text = node.attrs.get("value").unwrap();
        //     let galley = node.styling.get_font_galley(
        //         text,
        //         &self.renderer.taffy,
        //         &self.renderer.fonts,
        //         &parent.styling,
        //     );
        //     let cursor = galley.cursor_from_pos(relative_position);
        //     self.cursor_state.cursor = cursor;
        // }
    }

    pub fn on_mouse_input(
        &mut self,
        _renderer: &Renderer,
        button: &winit::event::MouseButton,
        state: &winit::event::ElementState,
    ) -> bool {
        let pressed_data = Arc::new(events::Event::Click(events::ClickEvent {
            state: self.state.clone(),
            button: button.clone(),
            pressed: true,
        }));

        let not_pressed_data = Arc::new(events::Event::Click(events::ClickEvent {
            state: self.state.clone(),
            button: button.clone(),
            pressed: false,
        }));

        for node_id in self.state.hovered.iter().copied() {
            match state {
                winit::event::ElementState::Pressed => {
                    self.send_event_to_element(node_id, "click", pressed_data.clone());
                    self.send_event_to_element(node_id, "mousedown", pressed_data.clone());
                }
                winit::event::ElementState::Released => {
                    self.send_event_to_element(node_id, "mouseup", not_pressed_data.clone());
                }
            }
        }

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

        self.state.focused = if let Some(node_id) = self.state.hovered.last().copied() {
            let node = FocusedNode {
                node_id,
                text_cursor: {
                    let node = self.tree.get_node_context_mut(node_id).unwrap();
                    if node.tag != "text".into() {
                        let node = self.tree.get_node_context_mut(node_id).unwrap();
                        let relative_pos =
                            self.state.cursor_state.current_position - node.computed.rect.min;

                        Some(
                            node.computed
                                .galley
                                .as_ref()
                                .unwrap()
                                .cursor_from_pos(relative_pos)
                                .ccursor
                                .index,
                        )
                    } else {
                        None
                    }
                },
            };

            self.send_event_to_element(
                node_id,
                "focus",
                Arc::new(events::Event::Focus(events::FocusEvent {
                    state: self.state.clone(),
                })),
            );

            Some(node)
        } else {
            None
        };

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
                if self.state.keyboard_state.modifiers.shift() {
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
}
