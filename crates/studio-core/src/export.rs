//! Crops, resizes and encodes a generated image for export, with the same
//! controls as Photoshop's Export As: format, quality, size, resampling and
//! pixel density.

use std::path::Path;

use image::codecs::png::{CompressionType, FilterType as PngFilter, PngEncoder};
use image::codecs::webp::WebPEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, ExtendedColorType, ImageEncoder, ImageReader};
use jpeg_encoder::{ColorType, Encoder, PixelDensity, SamplingFactor};
use serde::{Deserialize, Serialize};

/// Exported images may not be wider or taller than this many pixels.
pub const MAX_EDGE: u32 = 10_000;
/// Nor smaller than this.
pub const MIN_EDGE: u32 = 16;
/// The smallest crop, as a fraction of the image's width or height.
pub const MIN_CROP: f64 = 0.02;

/// A crop rectangle, in fractions of the image's width and height, so it
/// stays correct whatever the image's pixel size.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Crop {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Crop {
    pub fn validate(&self) -> Result<(), String> {
        let values = [self.x, self.y, self.width, self.height];
        let tolerance = 1e-6;
        let fits = values.iter().all(|value| value.is_finite())
            && self.x >= -tolerance
            && self.y >= -tolerance
            && self.width >= MIN_CROP - tolerance
            && self.height >= MIN_CROP - tolerance
            && self.x + self.width <= 1.0 + tolerance
            && self.y + self.height <= 1.0 + tolerance;
        if fits {
            Ok(())
        } else {
            Err("The crop must lie inside the image.".into())
        }
    }

    /// Whether this crop keeps the whole image.
    pub fn is_full(&self) -> bool {
        let tolerance = 1e-4;
        self.x.abs() < tolerance
            && self.y.abs() < tolerance
            && (self.width - 1.0).abs() < tolerance
            && (self.height - 1.0).abs() < tolerance
    }

    /// The crop in pixels of an image of the given size: left, top, width, height.
    pub fn pixels(&self, width: u32, height: u32) -> (u32, u32, u32, u32) {
        let edge = |start: f64, length: f64, total: u32| {
            let total_f = f64::from(total);
            let first = (start.clamp(0.0, 1.0) * total_f).round() as u32;
            let last = ((start + length).clamp(0.0, 1.0) * total_f).round() as u32;
            let first = first.min(total.saturating_sub(1));
            (first, last.clamp(first + 1, total) - first)
        };
        let (left, crop_width) = edge(self.x, self.width, width);
        let (top, crop_height) = edge(self.y, self.height, height);
        (left, top, crop_width, crop_height)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Jpeg,
    Png,
    Webp,
}

impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Png => "png",
            Self::Webp => "webp",
        }
    }
}

/// How pixels are recalculated when the size changes, with Photoshop's
/// closest equivalents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Resample {
    /// Sharpest; best for making images smaller ("Bicubic Sharper").
    Lanczos,
    /// Smooth; good for making images larger ("Bicubic Smoother").
    Bicubic,
    Bilinear,
    /// Hard pixel edges, no blending.
    Nearest,
}

impl Resample {
    fn filter(self) -> FilterType {
        match self {
            Self::Lanczos => FilterType::Lanczos3,
            Self::Bicubic => FilterType::CatmullRom,
            Self::Bilinear => FilterType::Triangle,
            Self::Nearest => FilterType::Nearest,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportOptions {
    pub format: Format,
    /// JPEG quality from 1 to 100. PNG and WebP are always lossless.
    pub quality: u8,
    /// Width in pixels; the height follows the image's proportions.
    /// `None` keeps the size of the (cropped) image.
    pub width: Option<u32>,
    pub resample: Resample,
    /// Pixels per inch recorded in JPEG files. Print shops usually ask for 300.
    pub dpi: u16,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            format: Format::Jpeg,
            quality: 92,
            width: None,
            resample: Resample::Lanczos,
            dpi: 72,
        }
    }
}

/// An encoded image, ready to be written to a file.
#[derive(Debug, Clone)]
pub struct Rendered {
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// Size of the image after cropping and before resizing.
    pub source_width: u32,
    pub source_height: u32,
}

/// Reads the image at `source`, applies the crop and the size, and encodes it.
pub fn render(
    source: &Path,
    crop: Option<&Crop>,
    options: &ExportOptions,
) -> Result<Rendered, String> {
    let image = ImageReader::open(source)
        .and_then(|reader| reader.with_guessed_format())
        .map_err(|e| format!("Cannot open {}: {e}", source.display()))?
        .decode()
        .map_err(|e| format!("Cannot read {}: {e}", source.display()))?;
    render_image(image, crop, options)
}

pub fn render_image(
    mut image: DynamicImage,
    crop: Option<&Crop>,
    options: &ExportOptions,
) -> Result<Rendered, String> {
    if let Some(crop) = crop.filter(|crop| !crop.is_full()) {
        crop.validate()?;
        let (left, top, width, height) = crop.pixels(image.width(), image.height());
        image = image.crop_imm(left, top, width, height);
    }
    let (source_width, source_height) = (image.width(), image.height());

    let (width, height) = target_size(source_width, source_height, options.width)?;
    if (width, height) != (source_width, source_height) {
        image = image.resize_exact(width, height, options.resample.filter());
    }

    let bytes = encode(&image, options)?;
    Ok(Rendered {
        bytes,
        width,
        height,
        source_width,
        source_height,
    })
}

/// The exported size for a requested width, keeping the proportions.
pub fn target_size(
    source_width: u32,
    source_height: u32,
    requested_width: Option<u32>,
) -> Result<(u32, u32), String> {
    let Some(width) = requested_width else {
        return Ok((source_width, source_height));
    };
    let ratio = f64::from(source_height) / f64::from(source_width.max(1));
    let height = (f64::from(width) * ratio).round().max(1.0) as u32;
    let allowed = MIN_EDGE..=MAX_EDGE;
    if !allowed.contains(&width) || !allowed.contains(&height) {
        return Err(format!(
            "That size would be {width} x {height} px. Both sides must be between {MIN_EDGE} and {MAX_EDGE} px."
        ));
    }
    Ok((width, height))
}

fn encode(image: &DynamicImage, options: &ExportOptions) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    let (width, height) = (image.width(), image.height());
    let failed = |e: &dyn std::fmt::Display| format!("Could not create the image file: {e}");

    match options.format {
        Format::Jpeg => {
            let quality = options.quality.clamp(1, 100);
            let rgb = image.to_rgb8();
            let mut encoder = Encoder::new(&mut bytes, quality);
            // Like Photoshop: full colour detail at high quality, the usual
            // half-resolution colour below it to save space.
            encoder.set_sampling_factor(if quality >= 90 {
                SamplingFactor::R_4_4_4
            } else {
                SamplingFactor::R_4_2_0
            });
            encoder.set_optimized_huffman_tables(true);
            encoder.set_density(PixelDensity::dpi(options.dpi.max(1)));
            let (width, height) = (u16::try_from(width), u16::try_from(height));
            let (Ok(width), Ok(height)) = (width, height) else {
                return Err(format!(
                    "Images larger than {MAX_EDGE} px cannot be exported."
                ));
            };
            encoder
                .encode(rgb.as_raw(), width, height, ColorType::Rgb)
                .map_err(|e| failed(&e))?;
        }
        Format::Png | Format::Webp => {
            let (raw, color) = if image.color().has_alpha() {
                (image.to_rgba8().into_raw(), ExtendedColorType::Rgba8)
            } else {
                (image.to_rgb8().into_raw(), ExtendedColorType::Rgb8)
            };
            let result = if options.format == Format::Png {
                PngEncoder::new_with_quality(&mut bytes, CompressionType::Best, PngFilter::Adaptive)
                    .write_image(&raw, width, height, color)
            } else {
                WebPEncoder::new_lossless(&mut bytes).write_image(&raw, width, height, color)
            };
            result.map_err(|e| failed(&e))?;
        }
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};

    /// A noisy test picture, so compression has real work to do.
    fn picture(width: u32, height: u32) -> DynamicImage {
        DynamicImage::ImageRgb8(RgbImage::from_fn(width, height, |x, y| {
            let noise = (x.wrapping_mul(7919) ^ y.wrapping_mul(104_729)) % 64;
            Rgb([(x % 256) as u8, (y % 256) as u8, (128 + noise) as u8])
        }))
    }

    fn jpeg(quality: u8) -> ExportOptions {
        ExportOptions {
            quality,
            ..ExportOptions::default()
        }
    }

    #[test]
    fn crops_are_checked_and_converted_to_pixels() {
        let half = Crop {
            x: 0.25,
            y: 0.5,
            width: 0.5,
            height: 0.5,
        };
        assert_eq!(half.validate(), Ok(()));
        assert_eq!(half.pixels(400, 200), (100, 100, 200, 100));
        assert!(!half.is_full());

        let full = Crop {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        };
        assert!(full.is_full());
        assert_eq!(full.pixels(3, 2), (0, 0, 3, 2));

        for bad in [
            Crop { x: -0.1, ..half },
            Crop { width: 0.9, ..half },
            Crop {
                height: 0.001,
                ..half
            },
            Crop {
                x: f64::NAN,
                ..half
            },
        ] {
            assert!(bad.validate().is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_crop_is_applied_before_resizing() {
        let crop = Crop {
            x: 0.5,
            y: 0.0,
            width: 0.5,
            height: 1.0,
        };
        let options = ExportOptions {
            width: Some(100),
            ..jpeg(90)
        };
        let rendered = render_image(picture(400, 300), Some(&crop), &options).unwrap();
        assert_eq!((rendered.source_width, rendered.source_height), (200, 300));
        assert_eq!((rendered.width, rendered.height), (100, 150));
    }

    #[test]
    fn jpeg_quality_density_and_colour_detail_follow_the_options() {
        let low = render_image(picture(256, 256), None, &jpeg(30)).unwrap();
        let high = render_image(picture(256, 256), None, &jpeg(100)).unwrap();
        assert_eq!(&high.bytes[..2], &[0xFF, 0xD8]);
        assert!(
            high.bytes.len() > low.bytes.len() * 2,
            "{} vs {}",
            high.bytes.len(),
            low.bytes.len()
        );

        let print = ExportOptions {
            dpi: 300,
            ..jpeg(92)
        };
        let bytes = render_image(picture(64, 64), None, &print).unwrap().bytes;
        // JFIF header: density unit 1 (inches), then 300 x 300.
        let jfif = bytes.windows(5).position(|w| w == b"JFIF\0").unwrap();
        assert_eq!(&bytes[jfif + 7..jfif + 12], &[1, 0x01, 0x2C, 0x01, 0x2C]);

        let decoded = image::load_from_memory(&high.bytes).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (256, 256));
    }

    #[test]
    fn png_and_webp_keep_every_pixel() {
        let original = picture(80, 60);
        for format in [Format::Png, Format::Webp] {
            let options = ExportOptions {
                format,
                ..ExportOptions::default()
            };
            let rendered = render_image(original.clone(), None, &options).unwrap();
            let decoded = image::load_from_memory(&rendered.bytes).unwrap();
            assert_eq!(decoded.to_rgb8(), original.to_rgb8(), "{format:?}");
        }
    }

    #[test]
    fn sizes_keep_proportions_and_stay_within_limits() {
        assert_eq!(target_size(2000, 1000, None), Ok((2000, 1000)));
        assert_eq!(target_size(2000, 1000, Some(500)), Ok((500, 250)));
        assert_eq!(target_size(1000, 2000, Some(3000)), Ok((3000, 6000)));
        assert!(target_size(1000, 2000, Some(6000)).is_err());
        assert!(target_size(2000, 100, Some(100)).is_err());
    }

    #[test]
    fn a_file_that_is_not_an_image_is_reported() {
        let path = std::env::temp_dir().join(format!("export-test-{}.png", std::process::id()));
        std::fs::write(&path, b"not an image").unwrap();
        assert!(render(&path, None, &ExportOptions::default()).is_err());
        std::fs::remove_file(&path).unwrap();
    }
}
