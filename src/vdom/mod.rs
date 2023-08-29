pub mod events;
pub mod tailwind;

use std::{
    ops::Deref,
    sync::{Arc, Mutex}, fmt::Debug,
};

use dioxus::{
    core::{BorrowedAttributeValue, ElementId, Mutations},
    prelude::{Element, Scope, TemplateAttribute, TemplateNode, VirtualDom},
};
use epaint::{ClippedShape, TextureId, WHITE_UV};
use rustc_hash::{FxHashMap, FxHashSet};
use slotmap::{new_key_type, HopSlotMap};
use smallvec::{smallvec, SmallVec};
use taffy::{Taffy, prelude::Size};
use winit::{dpi::{PhysicalPosition, PhysicalSize}, event_loop::EventLoopProxy};

use self::{events::{DomEvent}, tailwind::Tailwind};

new_key_type! { pub struct NodeId; }

#[derive(Debug)]
pub struct Node {
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
    event_listeners: FxHashMap<ElementId, SmallVec<[Arc<str>; 8]>>,
}

impl VDom {
    pub fn new() -> VDom {
        let mut nodes = HopSlotMap::with_key();
        let root_id = nodes.insert(Node {
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
                    if let Some(listeners) = self.event_listeners.get_mut(&id) {
                        listeners.push(name);
                    } else {
                        self.event_listeners.insert(id, smallvec![name]);
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
                let node = Node {
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
                let parent = self.nodes.insert(node);

                for child in children {
                    let child = self.create_template_node(child);
                    self.nodes[parent].children.push(child);
                }

                parent
            }
            TemplateNode::Text { text } => {
                let mut map = FxHashMap::default();
                map.insert(self.get_tag_or_attr_key("value"), text.to_string());

                self.nodes.insert(Node {
                    tag: "text".into(),
                    children: smallvec![],
                    attrs: map,
                    styling: None,
                })
            }

            _ => self.nodes.insert(Node {
                tag: "placeholder".into(),
                children: smallvec![],
                attrs: FxHashMap::default(),
            styling: None,
            }),
        }
    }

    pub fn get_root_id(&self) -> NodeId {
        self.element_id_mapping[&ElementId(0)]
    }

    pub fn traverse_tree(&self, root_id: NodeId, callback: &mut impl FnMut(&Node)) {
        let parent = self.nodes.get(root_id).unwrap();
        callback(parent);
        for child in parent.children.iter() {
            self.traverse_tree(*child, callback);
        }
    }

    pub fn traverse_tree_mut(&mut self, root_id: NodeId, callback: &mut impl FnMut(&mut Node)) {
        let mut children: [NodeId; MAX_CHILDREN] = [NodeId::default(); MAX_CHILDREN];
        let mut count = 0;
    
        {
            let parent = self.nodes.get_mut(root_id).unwrap();
            callback(parent);
            
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

#[derive(Default)]
pub struct Renderer {
    pub window_size: PhysicalSize<u32>,
}

pub struct DomEventLoop {
    vdom: Arc<Mutex<VDom>>,
    dom_event_sender: tokio::sync::mpsc::UnboundedSender<DomEvent>,
    pub pixels_per_point: f32,

    pub renderer: Renderer,
    pub last_cursor_pos: Option<PhysicalPosition<f64>>,
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
                                vdom.handle_event(name, data.deref().clone().into_any(), element_id, bubbles);
                            }
                        }

                        let mutations = vdom.render_immediate();
                        dbg!(&mutations);
                        render_vdom.lock().unwrap().apply_mutations(mutations);
                        
                        event_proxy.send_event(redraw_event_to_send.clone()).unwrap();
                    }
                });
            }
        });

        DomEventLoop {
            vdom: render_vdom,
            dom_event_sender,
            renderer: Renderer {
                window_size,
            },
            last_cursor_pos: None,
            pixels_per_point: 1.0,
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
        vdom.traverse_tree_mut(root_id, &mut |node| {
            let Some(class_attr) = node.attrs.get("class") else {
                return;
            };

            if let Some(styling) = &mut node.styling {
                styling.set_styling(taffy, class_attr);
            } else {
                let mut tw = Tailwind::default();
                tw.set_styling(taffy, class_attr);
                node.styling = Some(tw);
            }
        });

        vdom.traverse_tree(root_id, &mut |node| {
            let Some(styling) = &node.styling else {
                return;
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
        });

        let node = vdom.nodes.get(root_id).unwrap();
        let styling = node.styling.as_ref().unwrap();
        dbg!(self.renderer.window_size);
        taffy.compute_layout(styling.node.unwrap(), Size {
            width: taffy::style::AvailableSpace::Definite(self.renderer.window_size.width as f32),
            height: taffy::style::AvailableSpace::Definite(self.renderer.window_size.height as f32),
        }).unwrap()
    }

    pub fn get_paint_info(&mut self) -> Vec<ClippedShape> {
        self.calculate_layout();

        let root_id = self.vdom.lock().unwrap().get_root_id();
        let vdom = self.vdom.lock().unwrap();
        let mut location = epaint::Pos2::ZERO;
        // perhaps use VecDeque?
        let mut shapes = Vec::with_capacity(vdom.nodes.len());
        vdom.traverse_tree(root_id, &mut |node| {
            if let Some(styling) = &node.styling {
                let node = styling.node.unwrap();
                let layout = self.taffy.layout(node).unwrap();
                location += epaint::Vec2::new(layout.location.x, layout.location.y);
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
            }
        });

        shapes
    }

    /// bool: whether the window needs to be redrawn
    pub fn on_window_event(&mut self, event: &winit::event::WindowEvent<'_>) -> bool {
        use winit::event::WindowEvent;
        match event {
            WindowEvent::ScaleFactorChanged {
                scale_factor,
                new_inner_size,
            } => {
                self.pixels_per_point = *scale_factor as f32;
                self.renderer.window_size = **new_inner_size;
                true
            }

            WindowEvent::CursorMoved { position, .. } => self.on_mouse_move(position),

            WindowEvent::CursorLeft { .. } => {
                self.last_cursor_pos = None;
                false
            }

            // WindowEvent::MouseInput { state, button, .. } => {

            // }

            // WindowEvent::MouseInput { state, button, .. } => {}
            _ => false,
        }
    }

    pub fn get_elements_by_event(&self, event_listener: &str) {
        // self.event_
    }

    fn on_mouse_move(&mut self, pos_in_pixels: &PhysicalPosition<f64>) -> bool {
        let pos_in_points = epaint::pos2(
            pos_in_pixels.x as f32 / self.pixels_per_point,
            pos_in_pixels.y as f32 / self.pixels_per_point,
        );

        false
    }

    fn on_mouse_input(
        &mut self,
        state: winit::event::ElementState,
        button: winit::event::MouseButton,
    ) {

        // if state == tao::event::ElementState::Pressed {
        //     self.events.push(DomEvent { name: "mousedown", data: , element_id: (), bubbles: () })
        // }

        // self.events.push(DomEvent { name: "()", data: (), element_id: (), bubbles: () })
    }
}
