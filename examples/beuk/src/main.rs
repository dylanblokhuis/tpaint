#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release
#![allow(unsafe_code)]

use beuk::{ash::vk, ctx::RenderContextDescriptor};
use raw_window_handle::{HasRawDisplayHandle, HasRawWindowHandle};
use simple_logger::SimpleLogger;
use std::sync::Arc;
use tpaint::DomEventLoop;
use tpaint_beuk::{Renderer, ScreenDescriptor};
use winit::event::WindowEvent;

#[cfg(feature = "hot-reload")]
use tpaint::prelude::dioxus_hot_reload;

mod app;

type UserEvent = ();

fn main() {
    #[cfg(feature = "hot-reload")]
    dioxus_hot_reload::hot_reload_init!();

    SimpleLogger::new().init().unwrap();

    let event_loop = winit::event_loop::EventLoopBuilder::<UserEvent>::with_user_event().build();
    let window = winit::window::WindowBuilder::new()
        .with_decorations(true)
        .with_resizable(true)
        .with_transparent(false)
        .with_title("tpaint wgpu example")
        .with_inner_size(winit::dpi::PhysicalSize {
            width: 800,
            height: 600,
        })
        .build(&event_loop)
        .unwrap();

    let ctx = Arc::new(beuk::ctx::RenderContext::new(RenderContextDescriptor {
        display_handle: window.raw_display_handle(),
        window_handle: window.raw_window_handle(),
        present_mode: vk::PresentModeKHR::default(),
    }));

    let swapchain = ctx.get_swapchain();
    let mut renderer = Renderer::new(
        &ctx,
        swapchain.surface_format.format,
        swapchain.depth_image_format,
    );
    drop(swapchain);

    // let mut ctx = RenderContext::new(&device, config.format, None, 1, size);
    let mut app = DomEventLoop::spawn(
        app::app,
        window.inner_size(),
        window.scale_factor() as f32,
        event_loop.create_proxy(),
        (),
        (),
    );

    event_loop.run(move |event, _, control_flow| {
        let mut redraw = || {
            *control_flow = winit::event_loop::ControlFlow::Wait;

            let (primitives, delta, screen_descriptor) = app.get_paint_info();

            for (id, texture) in delta.set {
                renderer.update_texture(&ctx, id, &texture);
            }

            for id in delta.free {
                renderer.free_texture(&id);
            }

            let screen = &ScreenDescriptor {
                size_in_pixels: screen_descriptor.size.into(),
                pixels_per_point: screen_descriptor.pixels_per_point,
            };
            renderer.update_buffers(&ctx, &primitives);

            let present_index = ctx.acquire_present_index();

            ctx.present_record(
                present_index,
                |command_buffer, color_view, depth_view| unsafe {
                    let color_attachments = &[vk::RenderingAttachmentInfo::default()
                        .image_view(color_view)
                        .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                        .load_op(vk::AttachmentLoadOp::CLEAR)
                        .store_op(vk::AttachmentStoreOp::STORE)
                        .clear_value(vk::ClearValue {
                            color: vk::ClearColorValue {
                                float32: [0.1, 0.1, 0.1, 1.0],
                            },
                        })];

                    let depth_attachment = &vk::RenderingAttachmentInfo::default()
                        .image_view(depth_view)
                        .image_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL)
                        .load_op(vk::AttachmentLoadOp::CLEAR)
                        .store_op(vk::AttachmentStoreOp::STORE)
                        .clear_value(vk::ClearValue {
                            depth_stencil: vk::ClearDepthStencilValue {
                                depth: 1.0,
                                stencil: 0,
                            },
                        });

                    ctx.begin_rendering(command_buffer, color_attachments, Some(depth_attachment));
                    renderer.render(&ctx, &primitives, screen, command_buffer);
                    ctx.end_rendering(command_buffer);
                },
            );

            ctx.present_submit(present_index);
        };

        match event {
            winit::event::Event::RedrawEventsCleared if cfg!(target_os = "windows") => redraw(),
            winit::event::Event::RedrawRequested(_) if !cfg!(target_os = "windows") => redraw(),

            winit::event::Event::WindowEvent {
                event: ref window_event,
                ..
            } => {
                if matches!(
                    window_event,
                    WindowEvent::CloseRequested | WindowEvent::Destroyed
                ) {
                    *control_flow = winit::event_loop::ControlFlow::Exit;
                }

                if let winit::event::WindowEvent::Resized(physical_size) = &window_event {
                    ctx.recreate_swapchain(physical_size.width, physical_size.height);
                } else if let winit::event::WindowEvent::ScaleFactorChanged {
                    new_inner_size, ..
                } = &window_event
                {
                    ctx.recreate_swapchain(new_inner_size.width, new_inner_size.height);
                }

                let repaint = app.on_window_event(window_event);
                if repaint {
                    window.request_redraw();
                }
            }

            winit::event::Event::NewEvents(winit::event::StartCause::ResumeTimeReached {
                ..
            }) => {
                window.request_redraw();
            }
            winit::event::Event::UserEvent(_) => {
                window.request_redraw();
            }
            _ => {}
        }
    });
}
