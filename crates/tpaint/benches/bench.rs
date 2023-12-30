use criterion::{criterion_group, criterion_main, Criterion};
use dioxus::prelude::*;
use tpaint::prelude::*;
use tpaint::DomEventLoop;
use winit::{event_loop::EventLoopBuilder, window::WindowBuilder};

fn app(cx: Scope) -> Element {
    render! {
      view {
        class: "flex-col w-full p-10 gap-y-20 bg-slate-200 overflow-y-scroll",

        (0..100).map(|_| rsx! {
          view {
            class: "w-full h-50 p-10 bg-blue-900",
          }
        })
      }
    }
}

pub fn criterion_benchmark(_c: &mut Criterion) {
    let event_loop = EventLoopBuilder::with_user_event().build().unwrap();
    let window = WindowBuilder::new()
        .with_inner_size(winit::dpi::LogicalSize::new(800, 600))
        .build(&event_loop)
        .unwrap();

    let _app = DomEventLoop::spawn(
        app,
        window.inner_size(),
        window.scale_factor() as f32,
        event_loop.create_proxy(),
        (),
        (),
    );

    // c.bench_function("calculate_layout", |b| {
    //     b.iter(|| run_calculate_layout(black_box(&mut app)))
    // });

    // c.bench_function("get_paint_info", |b| {
    //     b.iter(|| run_paint_info(black_box(&mut app)))
    // });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
