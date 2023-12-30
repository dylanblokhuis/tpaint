use std::{sync::{Arc, Mutex}, fmt::Debug, ops::Deref};

use dioxus::prelude::{ScopeId, VirtualDom, Scope, Element};
use epaint::{text::FontDefinitions};


use winit::{dpi::{PhysicalSize}, event_loop::EventLoopProxy};


use crate::{
    events::{DomEvent,},
    renderer::{Renderer},
    dom::{Dom},
};



pub struct DomEventLoop {
    pub vdom: Arc<Mutex<Dom>>,
    dom_event_sender: tokio::sync::mpsc::UnboundedSender<DomEvent>,
    pub update_scope_sender: tokio::sync::mpsc::UnboundedSender<ScopeId>,
    pub renderer: Renderer,
}

impl DomEventLoop {

    pub fn spawn<E: Debug + Send + Sync + Clone, T: Clone + 'static + Send + Sync>(app: fn(Scope) -> Element, window_size: PhysicalSize<u32>, pixels_per_point: f32, event_proxy: EventLoopProxy<E>, redraw_event_to_send: E, root_context: T) -> DomEventLoop {
        let (dom_event_sender, mut dom_event_receiver) = tokio::sync::mpsc::unbounded_channel::<DomEvent>();
        let render_vdom = Arc::new(Mutex::new(Dom::new()));
    
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
        }
    }
}