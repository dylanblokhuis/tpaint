use std::{sync::{Arc, Mutex}, fmt::Debug, ops::Deref};

use dioxus::prelude::{ScopeId, VirtualDom, Scope, Element};
use epaint::{textures::TexturesDelta, ClippedPrimitive, TextureManager};
use winit::{event::WindowEvent, event_loop::EventLoopProxy, window::Window};


use crate::{
    events::DomEvent,
    renderer::{Renderer, RendererDescriptor, ScreenDescriptor},
    dom::Dom,
};

pub struct DomEventLoop {
    pub dom: Arc<Mutex<Dom>>,
    pub update_scope_sender: tokio::sync::mpsc::UnboundedSender<ScopeId>,
    pub renderer: Renderer,
}

#[derive(Clone)]
pub struct DomContext {
    pub texture_manager: Arc<Mutex<TextureManager>>,
    pub window: Arc<Window>,
    #[cfg(feature = "images")]
    pub client: reqwest::Client,
    pub event_sender: tokio::sync::mpsc::UnboundedSender<DomEvent>,
    pub current_cursor_icon: winit::window::CursorIcon,
}


impl DomEventLoop {
    pub fn spawn<E: Debug + Send + Sync + Clone, T: Clone + 'static + Send + Sync>(app: fn(Scope) -> Element, window: Arc<Window>, renderer_desc: RendererDescriptor, event_proxy: EventLoopProxy<E>, redraw_event_to_send: E, root_context: T) -> DomEventLoop {
        let (dom_event_sender, mut dom_event_receiver) = tokio::sync::mpsc::unbounded_channel::<DomEvent>();
      
        #[cfg(all(feature = "hot-reload", debug_assertions))]
        let (hot_reload_tx, mut hot_reload_rx) = tokio::sync::mpsc::unbounded_channel::<dioxus_hot_reload::HotReloadMsg>();
        #[cfg(not(all(feature = "hot-reload", debug_assertions)))]
        let (_, mut hot_reload_rx) = tokio::sync::mpsc::unbounded_channel::<()>();
    
        let (update_scope_sender, mut update_scope_receiver) = tokio::sync::mpsc::unbounded_channel::<ScopeId>();
        
        #[cfg(all(feature = "hot-reload", debug_assertions))]
        dioxus_hot_reload::connect(move |msg| {
            let _ = hot_reload_tx.send(msg);
        });
        let renderer = Renderer::new(renderer_desc);
        let dom_context = DomContext {
            texture_manager: renderer.tex_manager.clone(),
            window: window.clone(),
            #[cfg(feature = "images")]
            client: reqwest::Client::new(),
            event_sender: dom_event_sender.clone(),
            current_cursor_icon: Default::default(),
        };
        let dom = Arc::new(Mutex::new(Dom::new(dom_context.clone())));

        std::thread::spawn({
            let dom = dom.clone();
            let context = dom_context.clone();
            move || {
                let mut vdom = VirtualDom::new(app).with_root_context(root_context).with_root_context(context);
                let mutations = vdom.rebuild();
                dom.lock().unwrap().apply_mutations(mutations);
                event_proxy.send_event(redraw_event_to_send.clone()).unwrap();
    
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
                            dom.lock().unwrap().apply_mutations(mutations);
        
                            event_proxy.send_event(redraw_event_to_send.clone()).unwrap();
                        }
                    });
            }
        });
    
        DomEventLoop {
            dom,
            update_scope_sender,
            renderer,
        }
    }

    pub fn get_paint_info(&mut self) -> (Vec<ClippedPrimitive>, TexturesDelta, &ScreenDescriptor) {
        let mut vdom = self.dom.lock().unwrap();
        self.renderer.get_paint_info(&mut vdom)
    }

    pub fn on_window_event(&mut self, event: &winit::event::WindowEvent) -> bool {
        let mut repaint = false;

        match event {
            WindowEvent::Moved(position) => {
                let mut dom = self.dom.lock().unwrap();
                dom.on_window_moved(position);
                repaint = true;
            }
            WindowEvent::Resized(size) => {
                self.renderer.screen_descriptor = ScreenDescriptor {
                   size: *size,
                   pixels_per_point: self.renderer.screen_descriptor.pixels_per_point
                };
                let mut dom = self.dom.lock().unwrap();
                dom.on_window_resize();
                repaint = true;
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.renderer.screen_descriptor = ScreenDescriptor {
                    size: self.renderer.screen_descriptor.size,
                    pixels_per_point: *scale_factor as f32,
                };
                let mut dom = self.dom.lock().unwrap();
                dom.on_window_resize();
                repaint = true;
            }
            WindowEvent::MouseInput { button, state, .. } => {
                let mut dom = self.dom.lock().unwrap();
                repaint = dom.on_mouse_input(&self.renderer, button, state);
            }
            WindowEvent::CursorMoved { position, .. } => {
                let mut dom = self.dom.lock().unwrap();
                repaint = dom.on_mouse_move(position, &self.renderer.screen_descriptor);
            }
            WindowEvent::MouseWheel { delta,  .. } => {               
                let mut dom = self.dom.lock().unwrap();
                repaint = dom.on_scroll(delta)
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let mut dom = self.dom.lock().unwrap();
                repaint = dom.on_keyboard_input(event);
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                let mut dom = self.dom.lock().unwrap();
                dom.state.keyboard_state.modifiers = *modifiers;
            }
            WindowEvent::Focused(focused) => {
                let mut dom = self.dom.lock().unwrap();
                dom.state.keyboard_state.modifiers = Default::default();
                if !focused {
                    dom.set_focus(None);
                }
            }
            _ => {}
        }

        repaint
    }
}