use image::{DynamicImage, GrayImage};

/// 512px wide — leaves margin for TM-T88VI's non-printable edges on 80mm paper.
const PRINTER_WIDTH_PX: u32 = 512;

/// Preprocess an image for thermal printing:
/// 1. Decode from raw bytes (PNG, JPEG, etc.)
/// 2. Resize to printer width (576px), maintaining aspect ratio
/// 3. Convert to grayscale
/// 4. Apply gamma correction to lighten midtones (thermal printers darken)
/// 5. Floyd-Steinberg dithering to 1-bit
/// 6. Re-encode as PNG for escpos bit_image_from_bytes_option
pub fn preprocess_for_thermal(raw_bytes: &[u8]) -> Result<Vec<u8>, String> {
    let img =
        image::load_from_memory(raw_bytes).map_err(|e| format!("Image decode failed: {e}"))?;

    // Resize to printer width, maintaining aspect ratio
    let img = img.resize(
        PRINTER_WIDTH_PX,
        u32::MAX,
        image::imageops::FilterType::Lanczos3,
    );

    // Convert to grayscale
    let mut gray = img.to_luma8();

    // Gamma correction — lighten midtones for thermal printer
    apply_gamma(&mut gray, 1.8);

    // Floyd-Steinberg dithering
    floyd_steinberg_dither(&mut gray);

    // Re-encode as PNG
    let dithered = DynamicImage::ImageLuma8(gray);
    let mut buf = std::io::Cursor::new(Vec::new());
    dithered
        .write_to(&mut buf, image::ImageFormat::Png)
        .map_err(|e| format!("PNG encode failed: {e}"))?;

    Ok(buf.into_inner())
}

/// Apply gamma correction + Floyd-Steinberg dithering in-place on a grayscale image.
/// Call this on an already-resized `GrayImage` before encoding to PNG for escpos.
pub fn dither_for_thermal(img: &mut GrayImage) {
    apply_gamma(img, 1.5);
    floyd_steinberg_dither(img);
}

/// Apply gamma correction to lighten midtones.
/// gamma > 1.0 lightens the image (inverse gamma applied).
fn apply_gamma(img: &mut GrayImage, gamma: f32) {
    // Build a lookup table for speed
    let inv_gamma = 1.0 / gamma;
    let lut: Vec<u8> = (0..=255u16)
        .map(|v| {
            let normalized = v as f32 / 255.0;
            let corrected = normalized.powf(inv_gamma);
            (corrected * 255.0).round().min(255.0) as u8
        })
        .collect();

    for pixel in img.pixels_mut() {
        pixel[0] = lut[pixel[0] as usize];
    }
}

/// Floyd-Steinberg error-diffusion dithering.
/// Converts a grayscale image to 1-bit (0 or 255) in-place.
fn floyd_steinberg_dither(img: &mut GrayImage) {
    let width = img.width() as i32;
    let height = img.height() as i32;

    // Work with i16 buffer to handle error diffusion overflow
    let mut buf: Vec<i16> = img.pixels().map(|p| p[0] as i16).collect();

    for y in 0..height {
        for x in 0..width {
            let idx = (y * width + x) as usize;
            let old = buf[idx].clamp(0, 255);
            let new = if old > 127 { 255i16 } else { 0i16 };
            let err = old - new;
            buf[idx] = new;

            // Distribute error to neighbors
            if x + 1 < width {
                buf[(y * width + x + 1) as usize] += err * 7 / 16;
            }
            if y + 1 < height {
                if x > 0 {
                    buf[((y + 1) * width + x - 1) as usize] += err * 3 / 16;
                }
                buf[((y + 1) * width + x) as usize] += err * 5 / 16;
                if x + 1 < width {
                    buf[((y + 1) * width + x + 1) as usize] += err / 16;
                }
            }
        }
    }

    // Write back to image
    for (i, pixel) in img.pixels_mut().enumerate() {
        pixel[0] = buf[i].clamp(0, 255) as u8;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gamma_lightens_midtones() {
        let mut img = GrayImage::from_pixel(2, 2, image::Luma([128u8]));
        apply_gamma(&mut img, 1.8);
        // Gamma > 1.0 should lighten midtones (128 → higher value)
        assert!(img.get_pixel(0, 0)[0] > 128);
    }

    #[test]
    fn gamma_preserves_extremes() {
        let mut img = GrayImage::new(2, 1);
        img.put_pixel(0, 0, image::Luma([0u8]));
        img.put_pixel(1, 0, image::Luma([255u8]));
        apply_gamma(&mut img, 1.8);
        assert_eq!(img.get_pixel(0, 0)[0], 0);
        assert_eq!(img.get_pixel(1, 0)[0], 255);
    }

    #[test]
    fn dither_produces_only_black_and_white() {
        // Create a gradient image
        let mut img = GrayImage::new(100, 1);
        for x in 0..100 {
            img.put_pixel(x, 0, image::Luma([(x as f32 * 2.55) as u8]));
        }
        floyd_steinberg_dither(&mut img);
        for pixel in img.pixels() {
            assert!(
                pixel[0] == 0 || pixel[0] == 255,
                "Expected 0 or 255, got {}",
                pixel[0]
            );
        }
    }

    #[test]
    fn dither_white_stays_white() {
        let mut img = GrayImage::from_pixel(10, 10, image::Luma([255u8]));
        floyd_steinberg_dither(&mut img);
        for pixel in img.pixels() {
            assert_eq!(pixel[0], 255);
        }
    }

    #[test]
    fn dither_black_stays_black() {
        let mut img = GrayImage::from_pixel(10, 10, image::Luma([0u8]));
        floyd_steinberg_dither(&mut img);
        for pixel in img.pixels() {
            assert_eq!(pixel[0], 0);
        }
    }

    #[test]
    fn dither_midtone_has_mix() {
        // 50% gray should produce roughly 50% black/white pixels
        let mut img = GrayImage::from_pixel(100, 100, image::Luma([128u8]));
        floyd_steinberg_dither(&mut img);
        let black_count = img.pixels().filter(|p| p[0] == 0).count();
        let total = 10_000;
        // Should be roughly 50% ± 10%
        assert!(
            black_count > total * 40 / 100 && black_count < total * 60 / 100,
            "Expected ~50% black, got {}%",
            black_count * 100 / total
        );
    }
}
