use std::sync::Arc;

use dioxus::{
    core::{BorrowedAttributeValue, ElementId, Mutations},
    prelude::{TemplateAttribute, TemplateNode},
};
use epaint::Vec2;
use rustc_hash::{FxHashMap, FxHashSet};
use taffy::prelude::*;

use super::tailwind::{StyleState, Tailwind};

pub struct NodeContext {
    pub tag: Arc<str>,
    pub parent_id: Option<NodeId>,
    pub attrs: FxHashMap<Arc<str>, String>,
    pub styling: Tailwind,
    pub scroll: Vec2,
    pub natural_content_size: Size<f32>,
    pub computed_rect: epaint::Rect,
}

pub struct Dom {
    pub tree: TaffyTree<NodeContext>,
    templates: FxHashMap<String, Vec<NodeId>>,
    stack: Vec<NodeId>,
    pub element_id_mapping: FxHashMap<ElementId, NodeId>,
    common_tags_and_attr_keys: FxHashSet<Arc<str>>,
    pub event_listeners: FxHashMap<ElementId, Vec<Arc<str>>>,
}

impl Dom {
    pub fn new() -> Dom {
        let mut tree = TaffyTree::<NodeContext>::new();

        let mut tw = Tailwind::default();
        let style = tw.get_style("w-full h-full overflow-y-scroll flex-nowrap items-start justify-start scrollbar-default", &StyleState::default());

        let root_id = tree
            .new_leaf_with_context(
                style,
                NodeContext {
                    parent_id: None,
                    tag: "view".into(),
                    attrs: Default::default(),
                    styling: tw,
                    scroll: Default::default(),
                    natural_content_size: Default::default(),
                    computed_rect: epaint::Rect::ZERO,
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

    #[tracing::instrument(skip_all, name = "Dom::get_tag_or_attr_key")]
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

    #[tracing::instrument(skip_all, name = "Dom::create_template_node")]
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
                                Some((self.get_tag_or_attr_key(name), value.to_string()))
                            } else {
                                None
                            }
                        })
                        .collect(),
                    styling: Tailwind::default(),
                    scroll: Vec2::ZERO,
                    natural_content_size: Size::ZERO,
                    computed_rect: epaint::Rect::ZERO,
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
                attrs.insert(self.get_tag_or_attr_key("class"), "max-w-full".to_string());
                attrs.insert(self.get_tag_or_attr_key("value"), text.to_string());
                let mut node = NodeContext {
                    parent_id,
                    tag: "text".into(),
                    attrs,
                    styling: Tailwind::default(),
                    scroll: Vec2::ZERO,
                    natural_content_size: Size::ZERO,
                    computed_rect: epaint::Rect::ZERO,
                };
                let style = self.get_initial_styling(&mut node);
                let node_id = self.tree.new_leaf_with_context(style, node).unwrap();

                node_id
            }

            TemplateNode::Dynamic { .. } => {
                let mut attrs = FxHashMap::default();
                attrs.insert(self.get_tag_or_attr_key("class"), String::new());

                let node_id = self
                    .tree
                    .new_leaf_with_context(
                        Style::default(),
                        NodeContext {
                            parent_id,
                            tag: "view".into(),
                            attrs,
                            styling: Tailwind::default(),
                            scroll: Vec2::ZERO,
                            natural_content_size: Size::ZERO,
                            computed_rect: epaint::Rect::ZERO,
                        },
                    )
                    .unwrap();

                node_id
            }

            TemplateNode::DynamicText { .. } => {
                let mut attrs = FxHashMap::default();
                attrs.insert(self.get_tag_or_attr_key("value"), String::new());

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
                            natural_content_size: Size::ZERO,
                            computed_rect: epaint::Rect::ZERO,
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
                        natural_content_size: Size::ZERO,
                        computed_rect: epaint::Rect::ZERO,

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
                dioxus::core::Mutation::CreateTextNode { value, id } => {
                    let mut attrs = FxHashMap::default();
                    attrs.insert(self.get_tag_or_attr_key("value"), value.to_string());

                    let node = NodeContext {
                        parent_id: None,
                        attrs,
                        natural_content_size: Size::ZERO,
                        computed_rect: epaint::Rect::ZERO,

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
                    node.attrs.insert(key, value.to_string());
                }
                dioxus::core::Mutation::SetText { value, id } => {
                    let node_id = self.element_id_mapping[&id];
                    let key = self.get_tag_or_attr_key("value");
                    let node = self.tree.get_node_context_mut(node_id).unwrap();
                    node.attrs.insert(key, value.to_string());
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
    #[tracing::instrument(skip_all, name = "Dom::clone_node")]
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
            natural_content_size: Size::ZERO,
            computed_rect: epaint::Rect::ZERO,
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

    #[tracing::instrument(skip_all, name = "Dom::remove_node")]
    pub fn remove_node(&mut self, id: NodeId) {
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

    pub fn traverse_tree(
        &mut self,
        id: NodeId,
        callback: &mut impl FnMut((NodeId, &mut NodeContext)) -> bool,
    ) {
        let node: &mut NodeContext = self.tree.get_node_context_mut(id).unwrap();
        let should_continue = callback((id, node));
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
        callback: &mut impl FnMut(
            (NodeId, &mut NodeContext),
            Option<(NodeId, &mut NodeContext)>,
        ) -> bool,
    ) {
        if let Some(parent_id) = parent_id {
            let [node, parent] = self
                .tree
                .get_disjoint_node_context_mut([id, parent_id])
                .unwrap();
            let should_continue = callback((id, node), Some((parent_id, parent)));
            if !should_continue {
                return;
            }
        } else {
            let node = self.tree.get_node_context_mut(id).unwrap();
            let should_continue = callback((id, node), None);
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
        callback: &mut impl FnMut(&mut NodeContext, Option<&mut NodeContext>, &T) -> (bool, T),
    ) {
        let data = if let Some(parent_id) = parent_id {
            let [node, parent] = self
                .tree
                .get_disjoint_node_context_mut([id, parent_id])
                .unwrap();

            let (should_continue, new_data) = callback(node, Some(parent), data);
            if !should_continue {
                return;
            }

            new_data
        } else {
            let node = self.tree.get_node_context_mut(id).unwrap();
            let (should_continue, new_data) = callback(node, None, data);
            if !should_continue {
                return;
            }

            new_data
        };

        for child in self.tree.children(id).unwrap().iter() {
            self.traverse_tree_mut_with_parent_and_data(*child, Some(id), &data, callback);
        }
    }
}
