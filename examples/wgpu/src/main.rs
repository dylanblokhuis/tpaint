#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release
#![allow(unsafe_code)]

use std::sync::Arc;

use simple_logger::SimpleLogger;
use tpaint::{
    epaint::{
        text::{FontData, FontDefinitions},
        FontFamily,
    },
    DomEventLoop, RendererDescriptor,
};
use tpaint_wgpu::{Renderer, ScreenDescriptor};
use winit::event::WindowEvent;

#[cfg(feature = "hot-reload")]
use tpaint::prelude::dioxus_hot_reload;

mod app;

type UserEvent = ();

fn main() {
    #[cfg(feature = "hot-reload")]
    dioxus_hot_reload::hot_reload_init!();

    #[cfg(feature = "tracy")]
    let (chrome_layer, guard) = tracing_chrome::ChromeLayerBuilder::new().build();
    #[cfg(feature = "tracy")]
    use tracing_subscriber::layer::SubscriberExt;
    #[cfg(feature = "tracy")]
    tracing::subscriber::set_global_default(tracing_subscriber::registry().with(chrome_layer))
        .expect("set up the subscriber");

    SimpleLogger::new()
        .with_level(log::LevelFilter::Warn)
        .init()
        .unwrap();

    let event_loop = winit::event_loop::EventLoopBuilder::<UserEvent>::with_user_event()
        .build()
        .unwrap();
    let window = Arc::new(
        winit::window::WindowBuilder::new()
            .with_decorations(true)
            .with_resizable(true)
            .with_transparent(false)
            .with_title("tpaint wgpu example")
            .with_inner_size(winit::dpi::PhysicalSize {
                width: 800,
                height: 600,
            })
            .build(&event_loop)
            .unwrap(),
    );

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
    let surface = unsafe { instance.create_surface(&window).unwrap() };

    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: Some(&surface),
        force_fallback_adapter: false,
    }))
    .unwrap();

    let (device, queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            features: wgpu::Features::default(),
            limits: wgpu::Limits::default(),
            label: None,
        },
        None,
    ))
    .unwrap();

    let size = window.inner_size();

    let swapchain_capabilities = surface.get_capabilities(&adapter);
    let swapchain_format = swapchain_capabilities.formats[0];

    let mut config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format: swapchain_format,
        width: size.width,
        height: size.height,
        present_mode: wgpu::PresentMode::Fifo,
        alpha_mode: swapchain_capabilities.alpha_modes[0],
        view_formats: vec![],
    };
    surface.configure(&device, &config);

    let mut fonts = FontDefinitions::default();
    // Install my own font (maybe supporting non-latin characters):
    fonts.font_data.insert(
        "Inter-Regular".to_owned(),
        FontData::from_static(include_bytes!("../../example_ui/assets/Inter-Regular.ttf")),
    ); // .ttf and .otf supported

    // Put my font first (highest priority):
    fonts
        .families
        .get_mut(&FontFamily::Proportional)
        .unwrap()
        .insert(0, "Inter-Regular".to_owned());

    let mut renderer = Renderer::new(&device, swapchain_format, None, 1);
    let mut app = DomEventLoop::spawn(
        app::app,
        window.clone(),
        RendererDescriptor {
            window_size: window.inner_size(),
            pixels_per_point: window.scale_factor() as f32,
            font_definitions: fonts,
        },
        event_loop.create_proxy(),
        (),
        (),
    );

    event_loop
        .run(move |event, target| {
            // Have the closure take ownership of the resources.
            // `event_loop.run` never returns, therefore we must do this to ensure
            // the resources are properly cleaned up.
            let _ = (&instance, &adapter);

            let mut redraw = || {
                target.set_control_flow(winit::event_loop::ControlFlow::Wait);
                let frame = surface
                    .get_current_texture()
                    .expect("Failed to acquire next swap chain texture");
                let view = frame
                    .texture
                    .create_view(&wgpu::TextureViewDescriptor::default());
                let mut encoder =
                    device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

                let (primitives, delta, screen_descriptor) = app.get_paint_info();

                for (id, texture) in delta.set {
                    renderer.update_texture(&device, &queue, id, &texture);
                }

                for id in delta.free {
                    renderer.free_texture(&id);
                }

                let screen = &ScreenDescriptor {
                    size_in_pixels: screen_descriptor.size.into(),
                    pixels_per_point: screen_descriptor.pixels_per_point,
                };
                renderer.update_buffers(&device, &queue, &mut encoder, &primitives, screen);
                {
                    let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: None,
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &view,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        occlusion_query_set: None,
                        timestamp_writes: None,
                    });

                    renderer.render(&mut rpass, &primitives, screen)
                }

                queue.submit(Some(encoder.finish()));
                frame.present();
            };

            match event {
                // winit::event::Event::RedrawRequested if !cfg!(target_os = "windows") => redraw(),
                winit::event::Event::WindowEvent {
                    event: ref window_event,
                    ..
                } => {
                    match window_event {
                        WindowEvent::Resized(size) => {
                            config.width = size.width;
                            config.height = size.height;
                            surface.configure(&device, &config);
                            window.request_redraw();
                        }

                        WindowEvent::CloseRequested | WindowEvent::Destroyed => {
                            target.exit();
                        }

                        WindowEvent::RedrawRequested => {
                            redraw();
                        }

                        _ => {}
                    }

                    let repaint = app.on_window_event(window_event);
                    if repaint {
                        window.request_redraw();
                    }
                }

                winit::event::Event::UserEvent(_) => {
                    window.request_redraw();
                }
                _ => {}
            }
        })
        .unwrap();
}
