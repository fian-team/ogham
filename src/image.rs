use skia_safe::{images, AlphaType, ColorSpace, ColorType, ISize, Image, ImageInfo};
use std::collections::HashMap;

pub struct ImageCache(HashMap<String, Option<Image>>);

impl ImageCache {
    pub fn new() -> Self {
        Self(HashMap::new())
    }

    pub fn get(&mut self, path: &str) -> &Option<Image> {
        if let None = self.0.get(path) {
            let full_path = format!("data/assets/{}", path);
            let maybe_image = lodepng::decode32_file(full_path);
            if let Ok(image) = maybe_image {
                // Convert RGBA struct to raw bytes
                let raw_bytes: Vec<u8> = image
                    .buffer
                    .iter()
                    .flat_map(|rgba| [rgba.r, rgba.g, rgba.b, rgba.a])
                    .collect();
                let data = skia_safe::Data::new_copy(&raw_bytes);
                let image = images::raster_from_data(
                    &ImageInfo::new(
                        ISize::new(image.width as i32, image.height as i32),
                        ColorType::RGBA8888,
                        AlphaType::Premul,
                        ColorSpace::new_srgb_linear(),
                    ),
                    &data,
                    image.width * 4 as usize, // RGBA is 4 bytes per pixel
                );
                self.0.insert(path.to_string(), image);
            } else {
                self.0.insert(path.to_string(), None);
            }
        }
        &self.0.get(path).unwrap()
    }
}
