use skia_safe::{images, AlphaType, ColorSpace, ColorType, ISize, Image, ImageInfo};
use std::collections::HashMap;
use std::path::PathBuf;

pub struct ImageCache {
    images: HashMap<String, Option<Image>>,
    /// Base directory for image paths. `None` falls back to the historical
    /// `data/assets/` prefix, resolved against the process working directory.
    root: Option<PathBuf>,
}

impl ImageCache {
    pub fn new() -> Self {
        Self {
            images: HashMap::new(),
            root: None,
        }
    }

    /// Resolve image paths against `root` instead of `data/assets/` under
    /// the process working directory. Clears the cache so already-missed
    /// paths get another look under the new root.
    pub fn set_root(&mut self, root: impl Into<PathBuf>) {
        self.root = Some(root.into());
        self.images.clear();
    }

    pub fn get(&mut self, path: &str) -> &Option<Image> {
        if let None = self.images.get(path) {
            let full_path = match &self.root {
                Some(root) => root.join(path),
                None => PathBuf::from(format!("data/assets/{}", path)),
            };
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
                self.images.insert(path.to_string(), image);
            } else {
                self.images.insert(path.to_string(), None);
            }
        }
        &self.images.get(path).unwrap()
    }
}
