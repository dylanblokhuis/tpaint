use std::sync::Arc;

use dioxus::prelude::*;
use epaint::{TextureId, ColorImage, textures::TextureOptions};
use crate::{prelude::*, event_loop::DomContext};
use resvg::usvg::TreeParsing;

#[derive(Props, PartialEq, Clone, Debug)]
pub struct ImageProps<'a> {
  #[props(default = "", into)]
  pub class: &'a str,
  pub src: String,
}

pub fn Image<'a>(cx: Scope<'a, ImageProps<'a>>) -> Element {
  let dom_context = use_context::<DomContext>(cx).unwrap();
  let texture_id_state = use_state::<Option<TextureId>>(cx, || None);

  use_effect(cx, (&cx.props.src,), |(src,)| {
    to_owned![texture_id_state, dom_context];
    async move {
      let handle_png = |src: String, bytes: &[u8]| {
        let img = match image::load_from_memory(&bytes) {
          Ok(img) => img,
          Err(e) => {
            log::error!("Failed to load image in memory: {}", e);
            return;
          }
        };

        let size = [img.width() as usize, img.height() as usize];
        let rgba = img.to_rgba8();

        let id = dom_context.texture_manager.lock().unwrap().alloc(
            src,
            epaint::ImageData::Color(Arc::new(ColorImage::from_rgba_unmultiplied(size, &rgba))),
            TextureOptions::LINEAR,
        );
        texture_id_state.set(Some(id));
      };

      let handle_svg = |src: String, bytes: &[u8]| {
          
    
            let opt = resvg::usvg::Options::default();
            let rtree = resvg::usvg::Tree::from_data(&bytes, &opt)
                .map_err(|err| err.to_string())
                .expect("Failed to parse SVG file");

            let rtree = resvg::Tree::from_usvg(&rtree);
            let pixmap_size = rtree.size.to_int_size();
            let mut pixmap =
                resvg::tiny_skia::Pixmap::new(pixmap_size.width(), pixmap_size.height()).unwrap();
            rtree.render(resvg::tiny_skia::Transform::default(), &mut pixmap.as_mut());

            let texture_id = dom_context.texture_manager.lock().unwrap().alloc(
                src,
                epaint::ImageData::Color(Arc::new(ColorImage::from_rgba_unmultiplied(
                    [pixmap_size.width() as usize, pixmap_size.height() as usize],
                    pixmap.data(),
                ))),
                TextureOptions::LINEAR,
            );
            texture_id_state.set(Some(texture_id));
      };

      // if http:// or https://, use reqwest
      if src.starts_with("http://") || src.starts_with("https://") {
        let req = dom_context.client.get(&src).build().unwrap();
        let res = match dom_context.client.execute(req).await {
          Ok(res) => res,
          Err(e) => {
            log::error!("Failed to fetch URL inside image: {}", e);
            return;
          }
        };

        let is_svg = res.headers().get("content-type").map(|ct| ct.as_bytes().starts_with(b"image/svg+xml")).unwrap_or(false);

        let bytes = match res.bytes().await  {
          Ok(bytes) => bytes,
          Err(e) => {
            log::error!("Failed to decode body inside image: {}", e);
            return;
          }
        };

        if is_svg {
          handle_svg(src, &bytes);
        } else {
          handle_png(src, &bytes);
        }
      }
    }
  });

  let src = if let Some(texture_id) = texture_id_state.get() {
    Some(match texture_id {
      TextureId::Managed(uint) => uint,
      TextureId::User(uint) => uint,
    })
  } else {
    None
  };


  if let Some(src) = src {
    render! {
      view {
        class: "{cx.props.class}",
        src: "texture://{src}"
      }
    }
  } else {
    None
  }
}
