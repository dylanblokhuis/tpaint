use dioxus::{
    core::{BorrowedAttributeValue, ElementId, Mutations},
    prelude::{TemplateAttribute, TemplateNode},
};
use rustc_hash::FxHashMap;
use slotmap::{new_key_type, DenseSlotMap};

new_key_type! { pub struct NodeId; }

#[derive(Debug)]
pub struct Node {
    pub tag: String,
    pub attrs: FxHashMap<String, String>,
    pub children: Vec<NodeId>,
}

pub struct VDom {
    pub nodes: DenseSlotMap<NodeId, Node>,
    templates: FxHashMap<String, Vec<NodeId>>,
    stack: Vec<NodeId>,
    element_id_mapping: FxHashMap<ElementId, NodeId>,
}

impl VDom {
    pub fn new() -> VDom {
        let mut nodes = DenseSlotMap::with_key();
        let root_id = nodes.insert(Node {
            tag: "root".into(),
            attrs: FxHashMap::default(),
            children: vec![],
        });

        let mut element_id_mapping = FxHashMap::default();
        element_id_mapping.insert(ElementId(0), root_id);

        VDom {
            nodes,
            templates: FxHashMap::default(),
            stack: Vec::new(),
            element_id_mapping,
        }
    }

    pub fn apply_mutations(&mut self, mutations: Mutations) {
        for template in mutations.templates {
            let mut children = Vec::with_capacity(template.roots.len());
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
                    let new_nodes = self.stack.split_off(self.stack.len() - m);
                    let old_node_id = self.load_path(path);
                    let node = self.nodes.get_mut(old_node_id).unwrap();
                    node.children = new_nodes;
                }
                dioxus::core::Mutation::AppendChildren { m, id } => {
                    let children = self.stack.split_off(self.stack.len() - m);
                    println!("finding in map {:?}", id);
                    let parent = self.element_id_mapping[&id];
                    for child in children {
                        self.nodes[parent].children.push(child);
                    }
                }
                dioxus::core::Mutation::SetAttribute {
                    name, value, id, ..
                } => {
                    let node_id = self.element_id_mapping[&id];
                    let node = self.nodes.get_mut(node_id).unwrap();
                    // dbg!(&node.attrs);
                    if let BorrowedAttributeValue::None = &value {
                        node.attrs.remove(name);
                    } else {
                        node.attrs.insert(
                            name.to_string(),
                            match value {
                                BorrowedAttributeValue::Int(val) => val.to_string(),
                                BorrowedAttributeValue::Bool(val) => val.to_string(),
                                BorrowedAttributeValue::Float(val) => val.to_string(),
                                BorrowedAttributeValue::Text(val) => val.to_string(),
                                BorrowedAttributeValue::None => "".to_string(),
                                BorrowedAttributeValue::Any(val) => unimplemented!(),
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

    fn create_template_node(&mut self, node: &TemplateNode) -> NodeId {
        match *node {
            TemplateNode::Element {
                tag,
                attrs,
                children,
                ..
            } => {
                let parent = self.nodes.insert(Node {
                    tag: tag.to_string(),
                    attrs: attrs
                        .iter()
                        .filter_map(|val| {
                            if let TemplateAttribute::Static { name, value, .. } = val {
                                Some((name.to_string(), value.to_string()))
                            } else {
                                None
                            }
                        })
                        .collect(),
                    children: Vec::new(),
                });

                for child in children {
                    let child = self.create_template_node(child);
                    self.nodes[parent].children.push(child);
                }

                parent
            }
            TemplateNode::Text { text } => {
                let mut map = FxHashMap::default();
                map.insert("value".to_string(), text.to_string());

                self.nodes.insert(Node {
                    tag: "text".to_string(),
                    children: Vec::new(),
                    attrs: map,
                })
            }

            _ => self.nodes.insert(Node {
                tag: "placeholder".to_string(),
                children: Vec::new(),
                attrs: FxHashMap::default(),
            }),
        }
    }

    pub fn get_root_id(&self) -> NodeId {
        self.element_id_mapping[&ElementId(0)]
    }

    pub fn traverse_tree(&self, root_id: NodeId, callback: &impl Fn(&Node)) {
        let parent = self.nodes.get(root_id).unwrap();
        callback(parent);
        for child in parent.children.iter() {
            self.traverse_tree(*child, callback);
        }
    }
}
