//! Prepares a reference photo for upload: applies the camera's rotation tag,
//! shrinks very large photos, and re-encodes as JPEG so every request sends
//! the same predictable format.

use std::path::{Path, PathBuf};

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, ImageDecoder, ImageReader, RgbImage};

#[derive(Debug, Clone)]
pub struct ReferenceImage {
    pub mime_type: String,
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct PrepareOptions {
    /// Photos whose longer side exceeds this many pixels are scaled down.
    pub max_edge: u32,
    pub jpeg_quality: u8,
}

impl Default for PrepareOptions {
    fn default() -> Self {
        Self {
            max_edge: 2048,
            jpeg_quality: 92,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ReferenceError {
    #[error("cannot open {}: {source}", path.display())]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("cannot read {} as a photo (JPG, PNG and WebP are supported): {source}", path.display())]
    Decode {
        path: PathBuf,
        source: image::ImageError,
    },
}

pub fn prepare_reference(
    path: &Path,
    options: &PrepareOptions,
) -> Result<ReferenceImage, ReferenceError> {
    let io = |source| ReferenceError::Io {
        path: path.to_path_buf(),
        source,
    };
    let decode = |source| ReferenceError::Decode {
        path: path.to_path_buf(),
        source,
    };

    let reader = ImageReader::open(path)
        .map_err(io)?
        .with_guessed_format()
        .map_err(io)?;
    let mut decoder = reader.into_decoder().map_err(decode)?;
    // Phone cameras store portrait shots sideways plus a rotation tag.
    let orientation = decoder.orientation().map_err(decode)?;
    let mut photo = DynamicImage::from_decoder(decoder).map_err(decode)?;
    photo.apply_orientation(orientation);

    if photo.width().max(photo.height()) > options.max_edge {
        photo = photo.resize(options.max_edge, options.max_edge, FilterType::Lanczos3);
    }

    let rgb = flatten_on_white(&photo);
    let mut bytes = Vec::new();
    rgb.write_with_encoder(JpegEncoder::new_with_quality(
        &mut bytes,
        options.jpeg_quality,
    ))
    .map_err(decode)?;

    Ok(ReferenceImage {
        mime_type: "image/jpeg".into(),
        bytes,
        width: rgb.width(),
        height: rgb.height(),
    })
}

/// JPEG has no transparency, so transparent pixels are blended onto white
/// instead of turning black.
fn flatten_on_white(photo: &DynamicImage) -> RgbImage {
    if !photo.color().has_alpha() {
        return photo.to_rgb8();
    }
    let rgba = photo.to_rgba8();
    let mut rgb = RgbImage::new(rgba.width(), rgba.height());
    for (target, source) in rgb.pixels_mut().zip(rgba.pixels()) {
        let alpha = u32::from(source[3]);
        for channel in 0..3 {
            let blended = u32::from(source[channel]) * alpha + 255 * (255 - alpha);
            target[channel] = ((blended + 127) / 255) as u8;
        }
    }
    rgb
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    fn temp_file(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("studio-core-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn large_photos_are_scaled_down_and_become_jpeg() {
        let path = temp_file("large.png");
        RgbaImage::from_pixel(400, 200, Rgba([10, 20, 30, 255]))
            .save(&path)
            .unwrap();

        let options = PrepareOptions {
            max_edge: 100,
            jpeg_quality: 90,
        };
        let reference = prepare_reference(&path, &options).unwrap();

        assert_eq!((reference.width, reference.height), (100, 50));
        assert_eq!(reference.mime_type, "image/jpeg");
        assert_eq!(&reference.bytes[..2], &[0xFF, 0xD8]);
    }

    #[test]
    fn small_photos_keep_their_size() {
        let path = temp_file("small.png");
        RgbaImage::from_pixel(40, 30, Rgba([10, 20, 30, 255]))
            .save(&path)
            .unwrap();
        let reference = prepare_reference(&path, &PrepareOptions::default()).unwrap();
        assert_eq!((reference.width, reference.height), (40, 30));
    }

    #[test]
    fn transparent_pixels_become_white() {
        let transparent = DynamicImage::ImageRgba8(RgbaImage::from_pixel(2, 2, Rgba([0, 0, 0, 0])));
        let flattened = flatten_on_white(&transparent);
        assert_eq!(flattened.get_pixel(0, 0).0, [255, 255, 255]);
    }

    #[test]
    fn a_file_that_is_not_a_photo_is_reported() {
        let path = temp_file("notes.jpg");
        std::fs::write(&path, b"not an image").unwrap();
        let error = prepare_reference(&path, &PrepareOptions::default()).unwrap_err();
        assert!(matches!(error, ReferenceError::Decode { .. }));
    }
}
