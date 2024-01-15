#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use simple_logger::SimpleLogger;
use tpaint::DomEventLoop;
use tpaint_glow::painter::Painter;

#[cfg(feature = "hot-reload")]
use tpaint::prelude::dioxus_hot_reload;

mod app;

struct GlutinWindowContext {
    window: winit::window::Window,
    gl_context: glutin::context::PossiblyCurrentContext,
    gl_display: glutin::display::Display,
    gl_surface: glutin::surface::Surface<glutin::surface::WindowSurface>,
}

type UserEvent = ();

impl GlutinWindowContext {
    #[allow(unsafe_code)]
    unsafe fn new(event_loop: &winit::event_loop::EventLoopWindowTarget<UserEvent>) -> Self {
        use glutin::context::NotCurrentGlContextSurfaceAccessor;
        use glutin::display::GetGlDisplay;
        use glutin::display::GlDisplay;
        use glutin::prelude::GlSurface;
        use raw_window_handle::HasRawWindowHandle;
        let winit_window_builder = winit::window::WindowBuilder::new()
            .with_resizable(true)
            .with_inner_size(winit::dpi::LogicalSize {
                width: 800.0,
                height: 600.0,
            })
            .with_title("tpaint glow (OpenGL) example") // Keep hidden until we've painted something. See https://github.com/emilk/egui/pull/2279
            .with_visible(false);

        let config_template_builder = glutin::config::ConfigTemplateBuilder::new()
            .prefer_hardware_accelerated(Some(true))
            .with_depth_size(0)
            .with_stencil_size(0)
            .with_transparency(false);

        log::debug!("trying to get gl_config");
        let (mut window, gl_config) =
            glutin_winit::DisplayBuilder::new() // let glutin-winit helper crate handle the complex parts of opengl context creation
                .with_preference(glutin_winit::ApiPrefence::FallbackEgl) // https://github.com/emilk/egui/issues/2520#issuecomment-1367841150
                .with_window_builder(Some(winit_window_builder.clone()))
                .build(
                    event_loop,
                    config_template_builder,
                    |mut config_iterator| {
                        config_iterator.next().expect(
                            "failed to find a matching configuration for creating glutin config",
                        )
                    },
                )
                .expect("failed to create gl_config");
        let gl_display = gl_config.display();
        log::debug!("found gl_config: {:?}", &gl_config);

        let raw_window_handle = window.as_ref().map(|w| w.raw_window_handle());
        log::debug!("raw window handle: {:?}", raw_window_handle);
        let context_attributes =
            glutin::context::ContextAttributesBuilder::new().build(raw_window_handle);
        // by default, glutin will try to create a core opengl context. but, if it is not available, try to create a gl-es context using this fallback attributes
        let fallback_context_attributes = glutin::context::ContextAttributesBuilder::new()
            .with_context_api(glutin::context::ContextApi::Gles(None))
            .build(raw_window_handle);
        let not_current_gl_context = unsafe {
            gl_display
                    .create_context(&gl_config, &context_attributes)
                    .unwrap_or_else(|_| {
                        log::debug!("failed to create gl_context with attributes: {:?}. retrying with fallback context attributes: {:?}",
                            &context_attributes,
                            &fallback_context_attributes);
                        gl_config
                            .display()
                            .create_context(&gl_config, &fallback_context_attributes)
                            .expect("failed to create context even with fallback attributes")
                    })
        };

        // this is where the window is created, if it has not been created while searching for suitable gl_config
        let window = window.take().unwrap_or_else(|| {
            log::debug!("window doesn't exist yet. creating one now with finalize_window");
            glutin_winit::finalize_window(event_loop, winit_window_builder.clone(), &gl_config)
                .expect("failed to finalize glutin window")
        });
        let (width, height): (u32, u32) = window.inner_size().into();
        let width = std::num::NonZeroU32::new(if width == 0 { 1 } else { width }).unwrap();
        let height = std::num::NonZeroU32::new(if height == 0 { 1 } else { height }).unwrap();
        let surface_attributes =
            glutin::surface::SurfaceAttributesBuilder::<glutin::surface::WindowSurface>::new()
                .build(window.raw_window_handle(), width, height);
        log::debug!(
            "creating surface with attributes: {:?}",
            &surface_attributes
        );
        let gl_surface = unsafe {
            gl_display
                .create_window_surface(&gl_config, &surface_attributes)
                .unwrap()
        };
        log::debug!("surface created successfully: {gl_surface:?}.making context current");
        let gl_context = not_current_gl_context.make_current(&gl_surface).unwrap();

        gl_surface
            .set_swap_interval(
                &gl_context,
                glutin::surface::SwapInterval::Wait(std::num::NonZeroU32::new(1).unwrap()),
            )
            .unwrap();

        GlutinWindowContext {
            window,
            gl_context,
            gl_display,
            gl_surface,
        }
    }

    fn window(&self) -> &winit::window::Window {
        &self.window
    }

    fn resize(&self, physical_size: winit::dpi::PhysicalSize<u32>) {
        use glutin::surface::GlSurface;
        self.gl_surface.resize(
            &self.gl_context,
            physical_size.width.try_into().unwrap(),
            physical_size.height.try_into().unwrap(),
        );
    }

    fn swap_buffers(&self) -> glutin::error::Result<()> {
        use glutin::surface::GlSurface;
        self.gl_surface.swap_buffers(&self.gl_context)
    }

    fn get_proc_address(&self, addr: &std::ffi::CStr) -> *const std::ffi::c_void {
        use glutin::display::GlDisplay;
        self.gl_display.get_proc_address(addr)
    }
}

fn main() {
    #[cfg(feature = "hot-reload")]
    dioxus_hot_reload::hot_reload_init!();

    #[cfg(feature = "tracy")]
    {
        use tracing_subscriber::layer::SubscriberExt;
        tracing::subscriber::set_global_default(
            tracing_subscriber::registry().with(tracing_tracy::TracyLayer::new()),
        )
        .expect("set up the subscriber");
    }

    SimpleLogger::new().env().init().unwrap();

    let clear_color = [1.0, 1.0, 1.0];

    let event_loop = winit::event_loop::EventLoopBuilder::<UserEvent>::with_user_event().build();
    let (gl_window, gl) = create_display(&event_loop);
    let gl = std::sync::Arc::new(gl);

    let mut app = DomEventLoop::spawn(
        app::app,
        gl_window.window().inner_size(),
        gl_window.window().scale_factor() as f32,
        event_loop.create_proxy(),
        (),
        (),
    );
    let mut painter = Painter::new(gl.clone(), "", None)
        .map_err(|err| {
            log::error!("error occurred in initializing painter:\n{err}");
        })
        .unwrap();

    event_loop.run(move |event, _, control_flow| {
        let mut redraw = || {
            *control_flow = winit::event_loop::ControlFlow::Wait;
            unsafe {
                use glow::HasContext as _;
                gl.clear_color(clear_color[0], clear_color[1], clear_color[2], 1.0);
                gl.clear(glow::COLOR_BUFFER_BIT);
            }

            let (primitives, delta, screen_descriptor) = app.get_paint_info();

            for (id, image_delta) in delta.set {
                painter.set_texture(id, &image_delta);
            }

            painter.paint_primitives(
                screen_descriptor.size.into(),
                screen_descriptor.pixels_per_point,
                &primitives,
            );

            for id in delta.free {
                painter.free_texture(id);
            }

            gl_window.swap_buffers().unwrap();
            gl_window.window().set_visible(true);
        };

        match event {
            winit::event::Event::RedrawEventsCleared if cfg!(target_os = "windows") => redraw(),
            winit::event::Event::RedrawRequested(_) if !cfg!(target_os = "windows") => redraw(),

            winit::event::Event::WindowEvent { event, .. } => {
                use winit::event::WindowEvent;
                if matches!(event, WindowEvent::CloseRequested | WindowEvent::Destroyed) {
                    *control_flow = winit::event_loop::ControlFlow::Exit;
                }

                if let winit::event::WindowEvent::Resized(physical_size) = &event {
                    gl_window.resize(*physical_size);
                } else if let winit::event::WindowEvent::ScaleFactorChanged {
                    new_inner_size, ..
                } = &event
                {
                    gl_window.resize(**new_inner_size);
                }

                let repaint = app.on_window_event(&event);

                if repaint {
                    gl_window.window().request_redraw();
                }
            }
            winit::event::Event::LoopDestroyed => painter.destroy(),
            winit::event::Event::NewEvents(winit::event::StartCause::ResumeTimeReached {
                ..
            }) => {
                gl_window.window().request_redraw();
            }
            winit::event::Event::UserEvent(_) => {
                gl_window.window().request_redraw();
            }

            _ => (),
        }
    });
}

fn create_display(
    event_loop: &winit::event_loop::EventLoopWindowTarget<UserEvent>,
) -> (GlutinWindowContext, glow::Context) {
    let glutin_window_context = unsafe { GlutinWindowContext::new(event_loop) };
    let gl = unsafe {
        glow::Context::from_loader_function(|s| {
            let s = std::ffi::CString::new(s)
                .expect("failed to construct C string from string for gl proc address");

            glutin_window_context.get_proc_address(&s)
        })
    };

    (glutin_window_context, gl)
}
