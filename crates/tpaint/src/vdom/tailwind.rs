use std::collections::HashMap;

use epaint::emath::Align2;
use epaint::textures::TextureOptions;
use epaint::{Color32, ColorImage, FontFamily, FontId, Fonts, Rounding, TextureManager};
use lazy_static::lazy_static;
use log::debug;
use taffy::prelude::*;
use taffy::style::Style;

type Colors = HashMap<&'static str, HashMap<&'static str, [u8; 4]>>;

lazy_static! {
    static ref COLORS: Colors = {
        let mut colors = Colors::new();
        insert_default_colors(&mut colors);
        colors
    };
}

#[derive(Clone, PartialEq, Debug, Default)]
pub struct Border {
    pub color: Color32,
    pub width: f32,
    pub radius: Rounding,
}

#[derive(Clone, PartialEq, Debug)]
pub struct TextStyling {
    pub color: Color32,
    pub align: Align2,
    pub font: FontId,
}

impl Default for TextStyling {
    fn default() -> Self {
        Self {
            color: Color32::BLACK,
            align: Align2::LEFT_TOP,
            font: FontId {
                size: 16.0,
                family: FontFamily::default(),
            },
        }
    }
}

#[derive(Clone, PartialEq, Debug, Default)]
pub struct Tailwind {
    pub node: Option<taffy::tree::NodeId>,
    pub background_color: Color32,
    pub border: Border,
    pub text: TextStyling,
    pub texture_id: Option<epaint::TextureId>,
}

pub struct StyleState {
    pub hovered: bool,
    pub focused: bool,
}

impl Tailwind {
    pub fn set_styling(&mut self, taffy: &mut Taffy, class: &str, state: &StyleState) -> &mut Self {
        let classes = class.split_whitespace();

        // todo: perhaps find a way to this lazily
        let mut style = Style::default();
        self.background_color = Default::default();
        self.border = Default::default();
        self.text = Default::default();

        for class in classes {
            self.handle_class(&mut style, &COLORS, class);
            if state.hovered {
                if let Some(class) = class.strip_prefix("hover:") {
                    self.handle_class(&mut style, &COLORS, class);
                }
            }
            if state.focused {
                if let Some(class) = class.strip_prefix("focus:") {
                    self.handle_class(&mut style, &COLORS, class);
                }
            }
        }

        if let Some(node) = self.node {
            taffy.set_style(node, style).unwrap();
        } else {
            self.node = Some(taffy.new_leaf(style).unwrap());
        }

        self
    }

    pub fn set_texture(&mut self, taffy: &mut Taffy, src: &str, tex_manager: &mut TextureManager) {
        use usvg::TreeParsing;

        let mut path = std::path::PathBuf::new();
        path.push("assets");
        path.push(src);

        if self.texture_id.is_some() {
            self.set_aspect_ratio_layout(taffy, tex_manager);
            return;
        }

        let extension = path.extension().unwrap().to_str().unwrap();

        let id = if extension.contains("svg") {
            let opt = usvg::Options::default();
            let svg_bytes = std::fs::read(&path).unwrap();
            let rtree = usvg::Tree::from_data(&svg_bytes, &opt)
                .map_err(|err| err.to_string())
                .expect("Failed to parse SVG file");

            let rtree = resvg::Tree::from_usvg(&rtree);
            let pixmap_size = rtree.size.to_int_size();
            let mut pixmap =
                resvg::tiny_skia::Pixmap::new(pixmap_size.width(), pixmap_size.height()).unwrap();
            rtree.render(resvg::tiny_skia::Transform::default(), &mut pixmap.as_mut());

            tex_manager.alloc(
                path.to_string_lossy().to_string(),
                epaint::ImageData::Color(ColorImage::from_rgba_unmultiplied(
                    [pixmap_size.width() as usize, pixmap_size.height() as usize],
                    pixmap.data(),
                )),
                TextureOptions::LINEAR,
            )
        } else {
            let Ok(reader) = image::io::Reader::open(&path) else {
                log::error!("Failed to open image: {}", path.display());
                return;
            };
            let Ok(img) = reader.decode() else {
                log::error!("Failed to decode image: {}", path.display());
                return;
            };
            let size = [img.width() as usize, img.height() as usize];
            let rgba = img.to_rgba8();

            tex_manager.alloc(
                path.to_string_lossy().to_string(),
                epaint::ImageData::Color(ColorImage::from_rgba_unmultiplied(size, &rgba)),
                TextureOptions::LINEAR,
            )
        };

        self.texture_id = Some(id);
        self.set_aspect_ratio_layout(taffy, tex_manager);
    }

    fn set_aspect_ratio_layout(&self, taffy: &mut Taffy, tex_manager: &TextureManager) {
        let meta = tex_manager.meta(self.texture_id.unwrap()).unwrap();
        let image_size = [meta.size[0] as f32, meta.size[1] as f32];
        let aspect_ratio = image_size[0] / image_size[1];
        let mut style = taffy
            .style(
                self.node
                    .expect("set_image_default_sizing called before set_styling was called"),
            )
            .unwrap()
            .clone();

        if style.size.width == Dimension::AUTO && style.size.height == Dimension::AUTO {
            style.size.width = Dimension::Length(image_size[0]);
            style.size.height = Dimension::Length(image_size[1]);
        }
        // if we're scaling the height based on the new width
        else if style.size.width != Dimension::AUTO && style.size.height == Dimension::AUTO {
            let new_width = match style.size.width {
                Dimension::Length(val) => val,
                _ => image_size[0], // use old width if it's not a length
            };
            style.size.height = Dimension::Length(new_width / aspect_ratio);
        }
        // if we're scaling the width based on the new height
        else if style.size.height != Dimension::AUTO && style.size.width == Dimension::AUTO {
            let new_height = match style.size.height {
                Dimension::Length(val) => val,
                _ => image_size[1], // use old height if it's not a length
            };
            style.size.width = Dimension::Length(new_height * aspect_ratio);
        }

        taffy
            .set_style(
                self.node
                    .expect("set_image_default_sizing called before set_styling was called"),
                style,
            )
            .unwrap();
    }

    pub fn set_text_styling(
        &mut self,
        text: &str,
        taffy: &mut Taffy,
        fonts: &Fonts,
        parent: &Tailwind,
    ) {
        let shape = epaint::Shape::text(
            fonts,
            epaint::Pos2 { x: 0.0, y: 0.0 },
            epaint::emath::Align2::LEFT_TOP,
            text,
            parent.text.font.clone(),
            epaint::Color32::WHITE,
        );
        let rect = shape.visual_bounding_rect();
        let width = rect.width();

        let font_pad_width = parent.text.font.size / 7.5;
        let line_height = parent.text.font.size * 1.15;

        let style = Style {
            size: Size {
                width: Dimension::Length(width + font_pad_width),
                height: Dimension::Length(line_height),
            },
            ..Default::default()
        };

        if let Some(node) = self.node {
            taffy.set_style(node, style).unwrap();
        } else {
            self.node = Some(taffy.new_leaf(style).unwrap());
        }
    }

    fn handle_class(&mut self, style: &mut Style, colors: &Colors, class: &str) {
        if class == "flex-col" {
            style.display = Display::Flex;
            style.flex_direction = FlexDirection::Column;
        } else if class == "flex-row" {
            style.display = Display::Flex;
            style.flex_direction = FlexDirection::Row;
        }

        if let Some(class) = class.strip_prefix("w-") {
            style.size.width = handle_size(class);
        }

        if let Some(class) = class.strip_prefix("h-") {
            style.size.height = handle_size(class);
        }

        if let Some(class) = class.strip_prefix("bg-") {
            if let Some(color) = handle_color(class, colors) {
                self.background_color = color;
            }
        }

        if let Some(class) = class.strip_prefix("text-") {
            if let Some(color) = handle_color(class, colors) {
                self.text.color = color;
            }
        }

        if let Some(class) = class.strip_prefix("p-") {
            let padding = LengthPercentage::Length(class.parse::<f32>().unwrap_or(0.0));
            style.padding = Rect {
                top: padding,
                bottom: padding,
                left: padding,
                right: padding,
            }
        }

        if let Some(class) = class.strip_prefix("py-") {
            let padding = LengthPercentage::Length(class.parse::<f32>().unwrap_or(0.0));
            style.padding.top = padding;
            style.padding.bottom = padding;
        }

        if let Some(class) = class.strip_prefix("px-") {
            let padding = LengthPercentage::Length(class.parse::<f32>().unwrap_or(0.0));
            style.padding.left = padding;
            style.padding.right = padding;
        }

        if let Some(class) = class.strip_prefix("pt-") {
            let padding = LengthPercentage::Length(class.parse::<f32>().unwrap_or(0.0));
            style.padding.top = padding;
        }

        if let Some(class) = class.strip_prefix("pb-") {
            let padding = LengthPercentage::Length(class.parse::<f32>().unwrap_or(0.0));
            style.padding.bottom = padding;
        }

        if let Some(class) = class.strip_prefix("rounded-") {
            let value = class.parse::<f32>().unwrap_or(0.0);
            self.border.radius.ne = value;
            self.border.radius.nw = value;
            self.border.radius.se = value;
            self.border.radius.sw = value;
        }

        if let Some(class) = class.strip_prefix("border-") {
            if let Some(color) = handle_color(class, colors) {
                self.border.color = color;
            } else {
                let value = class.parse::<f32>().unwrap_or(0.0);
                self.border.width = value;
            }
        }

        if let Some(class) = class.strip_prefix("justify-") {
            style.justify_content = Some(match class {
                "start" => JustifyContent::Start,
                "end" => JustifyContent::End,
                "center" => JustifyContent::Center,
                "between" => JustifyContent::SpaceBetween,
                "around" => JustifyContent::SpaceAround,
                "evenly" => JustifyContent::SpaceEvenly,
                "stretch" => JustifyContent::Stretch,
                _ => panic!("Unknown justify content {class}"),
            })
        }

        if let Some(class) = class.strip_prefix("items-") {
            match class {
                "start" => style.align_items = Some(AlignItems::FlexStart),
                "end" => style.align_items = Some(AlignItems::FlexEnd),
                "center" => style.align_items = Some(AlignItems::Center),
                "baseline" => style.align_items = Some(AlignItems::Baseline),
                "stretch" => style.align_items = Some(AlignItems::Stretch),
                _ => debug!("Unknown align items {class}"),
            }
        }

        if let Some(class) = class.strip_prefix("flex-") {
            match class {
                "wrap" => style.flex_wrap = FlexWrap::Wrap,
                "wrap-reverse" => style.flex_wrap = FlexWrap::WrapReverse,
                "nowrap" => style.flex_wrap = FlexWrap::NoWrap,
                _ => {}
            }
        }

        if let Some(class) = class.strip_prefix("gap-") {
            let gap = LengthPercentage::Length(class.parse::<f32>().unwrap_or(0.0));
            style.gap = Size {
                width: gap,
                height: gap,
            };
        }

        if let Some(class) = class.strip_prefix("gap-x-") {
            let gap = LengthPercentage::Length(class.parse::<f32>().unwrap_or(0.0));
            style.gap.width = gap;
        }

        if let Some(class) = class.strip_prefix("gap-y-") {
            let gap = LengthPercentage::Length(class.parse::<f32>().unwrap_or(0.0));
            style.gap.height = gap;
        }

        if class == "relative" {
            style.position = Position::Relative;
        }

        if class == "absolute" {
            style.position = Position::Absolute;
        }

        if class == "hidden" {
            style.display = Display::None;
        }

        if let Some(class) = class.strip_prefix("left-") {
            style.inset.left = LengthPercentageAuto::Length(class.parse::<f32>().unwrap_or(0.0));
        }
        if let Some(class) = class.strip_prefix("right-") {
            style.inset.right = LengthPercentageAuto::Length(class.parse::<f32>().unwrap_or(0.0));
        }

        if let Some(class) = class.strip_prefix("top-") {
            style.inset.top = LengthPercentageAuto::Length(class.parse::<f32>().unwrap_or(0.0));
        }

        if let Some(class) = class.strip_prefix("bottom-") {
            style.inset.bottom = LengthPercentageAuto::Length(class.parse::<f32>().unwrap_or(0.0));
        }
    }
}

fn handle_size(class: &str) -> Dimension {
    match class {
        "full" => Dimension::Percent(1.0),
        "auto" => Dimension::AUTO,
        class => {
            if class.ends_with('%') {
                Dimension::Percent(
                    class
                        .strip_suffix('%')
                        .unwrap()
                        .parse::<f32>()
                        .unwrap_or(0.0)
                        / 100.0,
                )
            } else {
                Dimension::Length(class.parse::<f32>().unwrap_or(0.0))
            }
        }
    }
}

fn handle_color(class: &str, colors: &Colors) -> Option<Color32> {
    // Split the class into components
    let components: Vec<&str> = class.split('/').collect();
    let color_and_variant: Vec<&str> = components[0].split('-').collect();

    // If there's an alpha channel specified, get it
    let alpha = if components.len() > 1 {
        match components[1].parse::<u16>() {
            // convert from 100 to 255k
            Ok(a) => (a * 255 / 100) as u8,
            Err(_) => return None, // Invalid alpha
        }
    } else {
        255 // Default alpha
    };

    // Handle special colors
    if color_and_variant.len() == 1 {
        return match color_and_variant[0] {
            "transparent" => Some(Color32::from_rgba_unmultiplied(0, 0, 0, 0)),
            "white" => Some(Color32::from_rgba_unmultiplied(255, 255, 255, alpha)),
            "black" => Some(Color32::from_rgba_unmultiplied(0, 0, 0, alpha)),
            _ => colors.get(color_and_variant[0]).map(|c| {
                let (_, variant) = c.iter().next().unwrap();
                Color32::from_rgba_unmultiplied(variant[0], variant[1], variant[2], alpha)
            }),
        };
    }

    // Handle regular colors
    let color = color_and_variant[0];
    let variant = color_and_variant[1];

    colors.get(color).and_then(|variants| {
        variants
            .get(variant)
            .map(|&[r, g, b, _]| Color32::from_rgba_unmultiplied(r, g, b, alpha))
    })
}

pub fn insert_default_colors(colors: &mut Colors) {
    colors.insert(
        "slate",
        vec![
            ("50", [248, 250, 252, 255]),
            ("100", [241, 245, 249, 255]),
            ("200", [226, 232, 240, 255]),
            ("300", [203, 213, 225, 255]),
            ("400", [148, 163, 184, 255]),
            ("500", [100, 116, 139, 255]),
            ("600", [71, 85, 105, 255]),
            ("700", [51, 65, 85, 255]),
            ("800", [30, 41, 59, 255]),
            ("900", [15, 23, 42, 255]),
            ("950", [2, 6, 23, 255]),
        ]
        .into_iter()
        .collect(),
    );
    colors.insert(
        "gray",
        vec![
            ("50", [249, 250, 251, 255]),
            ("100", [243, 244, 246, 255]),
            ("200", [229, 231, 235, 255]),
            ("300", [209, 213, 219, 255]),
            ("400", [156, 163, 175, 255]),
            ("500", [107, 114, 128, 255]),
            ("600", [75, 85, 99, 255]),
            ("700", [55, 65, 81, 255]),
            ("800", [31, 41, 55, 255]),
            ("900", [17, 24, 39, 255]),
            ("950", [3, 7, 18, 255]),
        ]
        .into_iter()
        .collect(),
    );
    colors.insert(
        "zinc",
        vec![
            ("50", [250, 250, 250, 255]),
            ("100", [244, 244, 245, 255]),
            ("200", [228, 228, 231, 255]),
            ("300", [212, 212, 216, 255]),
            ("400", [161, 161, 170, 255]),
            ("500", [113, 113, 122, 255]),
            ("600", [82, 82, 91, 255]),
            ("700", [63, 63, 70, 255]),
            ("800", [39, 39, 42, 255]),
            ("900", [24, 24, 27, 255]),
            ("950", [9, 9, 11, 255]),
        ]
        .into_iter()
        .collect(),
    );
    colors.insert(
        "neutral",
        vec![
            ("50", [250, 250, 250, 255]),
            ("100", [245, 245, 245, 255]),
            ("200", [229, 229, 229, 255]),
            ("300", [212, 212, 212, 255]),
            ("400", [163, 163, 163, 255]),
            ("500", [115, 115, 115, 255]),
            ("600", [82, 82, 82, 255]),
            ("700", [64, 64, 64, 255]),
            ("800", [38, 38, 38, 255]),
            ("900", [23, 23, 23, 255]),
            ("950", [10, 10, 10, 255]),
        ]
        .into_iter()
        .collect(),
    );
    colors.insert(
        "stone",
        vec![
            ("50", [250, 250, 249, 255]),
            ("100", [245, 245, 244, 255]),
            ("200", [231, 229, 228, 255]),
            ("300", [214, 211, 209, 255]),
            ("400", [168, 162, 158, 255]),
            ("500", [120, 113, 108, 255]),
            ("600", [87, 83, 78, 255]),
            ("700", [68, 64, 60, 255]),
            ("800", [41, 37, 36, 255]),
            ("900", [28, 25, 23, 255]),
            ("950", [12, 10, 9, 255]),
        ]
        .into_iter()
        .collect(),
    );
    colors.insert(
        "red",
        vec![
            ("50", [254, 242, 242, 255]),
            ("100", [254, 226, 226, 255]),
            ("200", [254, 202, 202, 255]),
            ("300", [252, 165, 165, 255]),
            ("400", [248, 113, 113, 255]),
            ("500", [239, 68, 68, 255]),
            ("600", [220, 38, 38, 255]),
            ("700", [185, 28, 28, 255]),
            ("800", [153, 27, 27, 255]),
            ("900", [127, 29, 29, 255]),
            ("950", [69, 10, 10, 255]),
        ]
        .into_iter()
        .collect(),
    );
    colors.insert(
        "orange",
        vec![
            ("50", [255, 247, 237, 255]),
            ("100", [255, 237, 213, 255]),
            ("200", [254, 215, 170, 255]),
            ("300", [253, 186, 116, 255]),
            ("400", [251, 146, 60, 255]),
            ("500", [249, 115, 22, 255]),
            ("600", [234, 88, 12, 255]),
            ("700", [194, 65, 12, 255]),
            ("800", [154, 52, 18, 255]),
            ("900", [124, 45, 18, 255]),
            ("950", [67, 20, 7, 255]),
        ]
        .into_iter()
        .collect(),
    );
    colors.insert(
        "amber",
        vec![
            ("50", [255, 251, 235, 255]),
            ("100", [254, 243, 199, 255]),
            ("200", [253, 230, 138, 255]),
            ("300", [252, 211, 77, 255]),
            ("400", [251, 191, 36, 255]),
            ("500", [245, 158, 11, 255]),
            ("600", [217, 119, 6, 255]),
            ("700", [180, 83, 9, 255]),
            ("800", [146, 64, 14, 255]),
            ("900", [120, 53, 15, 255]),
            ("950", [69, 26, 3, 255]),
        ]
        .into_iter()
        .collect(),
    );
    colors.insert(
        "yellow",
        vec![
            ("50", [254, 252, 232, 255]),
            ("100", [254, 249, 195, 255]),
            ("200", [254, 240, 138, 255]),
            ("300", [253, 224, 71, 255]),
            ("400", [250, 204, 21, 255]),
            ("500", [234, 179, 8, 255]),
            ("600", [202, 138, 4, 255]),
            ("700", [161, 98, 7, 255]),
            ("800", [133, 77, 14, 255]),
            ("900", [113, 63, 18, 255]),
            ("950", [66, 32, 6, 255]),
        ]
        .into_iter()
        .collect(),
    );
    colors.insert(
        "lime",
        vec![
            ("50", [247, 254, 231, 255]),
            ("100", [236, 252, 203, 255]),
            ("200", [217, 249, 157, 255]),
            ("300", [190, 242, 100, 255]),
            ("400", [163, 230, 53, 255]),
            ("500", [132, 204, 22, 255]),
            ("600", [101, 163, 13, 255]),
            ("700", [77, 124, 15, 255]),
            ("800", [63, 98, 18, 255]),
            ("900", [54, 83, 20, 255]),
            ("950", [26, 46, 5, 255]),
        ]
        .into_iter()
        .collect(),
    );
    colors.insert(
        "green",
        vec![
            ("50", [240, 253, 244, 255]),
            ("100", [220, 252, 231, 255]),
            ("200", [187, 247, 208, 255]),
            ("300", [134, 239, 172, 255]),
            ("400", [74, 222, 128, 255]),
            ("500", [34, 197, 94, 255]),
            ("600", [22, 163, 74, 255]),
            ("700", [21, 128, 61, 255]),
            ("800", [22, 101, 52, 255]),
            ("900", [20, 83, 45, 255]),
            ("950", [5, 46, 22, 255]),
        ]
        .into_iter()
        .collect(),
    );
    colors.insert(
        "emerald",
        vec![
            ("50", [236, 253, 245, 255]),
            ("100", [209, 250, 229, 255]),
            ("200", [167, 243, 208, 255]),
            ("300", [110, 231, 183, 255]),
            ("400", [52, 211, 153, 255]),
            ("500", [16, 185, 129, 255]),
            ("600", [5, 150, 105, 255]),
            ("700", [4, 120, 87, 255]),
            ("800", [6, 95, 70, 255]),
            ("900", [6, 78, 59, 255]),
            ("950", [2, 44, 34, 255]),
        ]
        .into_iter()
        .collect(),
    );
    colors.insert(
        "teal",
        vec![
            ("50", [240, 253, 250, 255]),
            ("100", [204, 251, 241, 255]),
            ("200", [153, 246, 228, 255]),
            ("300", [94, 234, 212, 255]),
            ("400", [45, 212, 191, 255]),
            ("500", [20, 184, 166, 255]),
            ("600", [13, 148, 136, 255]),
            ("700", [15, 118, 110, 255]),
            ("800", [17, 94, 89, 255]),
            ("900", [19, 78, 74, 255]),
            ("950", [4, 47, 46, 255]),
        ]
        .into_iter()
        .collect(),
    );
    colors.insert(
        "cyan",
        vec![
            ("50", [236, 254, 255, 255]),
            ("100", [207, 250, 254, 255]),
            ("200", [165, 243, 252, 255]),
            ("300", [103, 232, 249, 255]),
            ("400", [34, 211, 238, 255]),
            ("500", [6, 182, 212, 255]),
            ("600", [8, 145, 178, 255]),
            ("700", [14, 116, 144, 255]),
            ("800", [21, 94, 117, 255]),
            ("900", [22, 78, 99, 255]),
            ("950", [8, 51, 68, 255]),
        ]
        .into_iter()
        .collect(),
    );
    colors.insert(
        "sky",
        vec![
            ("50", [240, 249, 255, 255]),
            ("100", [224, 242, 254, 255]),
            ("200", [186, 230, 253, 255]),
            ("300", [125, 211, 252, 255]),
            ("400", [56, 189, 248, 255]),
            ("500", [14, 165, 233, 255]),
            ("600", [2, 132, 199, 255]),
            ("700", [3, 105, 161, 255]),
            ("800", [7, 89, 133, 255]),
            ("900", [12, 74, 110, 255]),
            ("950", [8, 47, 73, 255]),
        ]
        .into_iter()
        .collect(),
    );
    colors.insert(
        "blue",
        vec![
            ("50", [239, 246, 255, 255]),
            ("100", [219, 234, 254, 255]),
            ("200", [191, 219, 254, 255]),
            ("300", [147, 197, 253, 255]),
            ("400", [96, 165, 250, 255]),
            ("500", [59, 130, 246, 255]),
            ("600", [37, 99, 235, 255]),
            ("700", [29, 78, 216, 255]),
            ("800", [30, 64, 175, 255]),
            ("900", [30, 58, 138, 255]),
            ("950", [23, 37, 84, 255]),
        ]
        .into_iter()
        .collect(),
    );
    colors.insert(
        "indigo",
        vec![
            ("50", [238, 242, 255, 255]),
            ("100", [224, 231, 255, 255]),
            ("200", [199, 210, 254, 255]),
            ("300", [165, 180, 252, 255]),
            ("400", [129, 140, 248, 255]),
            ("500", [99, 102, 241, 255]),
            ("600", [79, 70, 229, 255]),
            ("700", [67, 56, 202, 255]),
            ("800", [55, 48, 163, 255]),
            ("900", [49, 46, 129, 255]),
            ("950", [30, 27, 75, 255]),
        ]
        .into_iter()
        .collect(),
    );
    colors.insert(
        "violet",
        vec![
            ("50", [245, 243, 255, 255]),
            ("100", [237, 233, 254, 255]),
            ("200", [221, 214, 254, 255]),
            ("300", [196, 181, 253, 255]),
            ("400", [167, 139, 250, 255]),
            ("500", [139, 92, 246, 255]),
            ("600", [124, 58, 237, 255]),
            ("700", [109, 40, 217, 255]),
            ("800", [91, 33, 182, 255]),
            ("900", [76, 29, 149, 255]),
            ("950", [46, 16, 101, 255]),
        ]
        .into_iter()
        .collect(),
    );
    colors.insert(
        "purple",
        vec![
            ("50", [250, 245, 255, 255]),
            ("100", [243, 232, 255, 255]),
            ("200", [233, 213, 255, 255]),
            ("300", [216, 180, 254, 255]),
            ("400", [192, 132, 252, 255]),
            ("500", [168, 85, 247, 255]),
            ("600", [147, 51, 234, 255]),
            ("700", [126, 34, 206, 255]),
            ("800", [107, 33, 168, 255]),
            ("900", [88, 28, 135, 255]),
            ("950", [59, 7, 100, 255]),
        ]
        .into_iter()
        .collect(),
    );
    colors.insert(
        "fuchsia",
        vec![
            ("50", [253, 244, 255, 255]),
            ("100", [250, 232, 255, 255]),
            ("200", [245, 208, 254, 255]),
            ("300", [240, 171, 252, 255]),
            ("400", [232, 121, 249, 255]),
            ("500", [217, 70, 239, 255]),
            ("600", [192, 38, 211, 255]),
            ("700", [162, 28, 175, 255]),
            ("800", [134, 25, 143, 255]),
            ("900", [112, 26, 117, 255]),
            ("950", [74, 4, 78, 255]),
        ]
        .into_iter()
        .collect(),
    );
    colors.insert(
        "pink",
        vec![
            ("50", [253, 242, 248, 255]),
            ("100", [252, 231, 243, 255]),
            ("200", [251, 207, 232, 255]),
            ("300", [249, 168, 212, 255]),
            ("400", [244, 114, 182, 255]),
            ("500", [236, 72, 153, 255]),
            ("600", [219, 39, 119, 255]),
            ("700", [190, 24, 93, 255]),
            ("800", [157, 23, 77, 255]),
            ("900", [131, 24, 67, 255]),
            ("950", [80, 7, 36, 255]),
        ]
        .into_iter()
        .collect(),
    );
    colors.insert(
        "rose",
        vec![
            ("50", [255, 241, 242, 255]),
            ("100", [255, 228, 230, 255]),
            ("200", [254, 205, 211, 255]),
            ("300", [253, 164, 175, 255]),
            ("400", [251, 113, 133, 255]),
            ("500", [244, 63, 94, 255]),
            ("600", [225, 29, 72, 255]),
            ("700", [190, 18, 60, 255]),
            ("800", [159, 18, 57, 255]),
            ("900", [136, 19, 55, 255]),
            ("950", [76, 5, 25, 255]),
        ]
        .into_iter()
        .collect(),
    );
}
