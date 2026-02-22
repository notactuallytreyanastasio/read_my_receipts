use image::{DynamicImage, GrayImage};

/// 512px wide — leaves margin for TM-T88VI's non-printable edges on 80mm paper.
const PRINTER_WIDTH_PX: u32 = 512;

/// Preprocess an image for thermal printing:
/// 1. Decode from raw bytes (PNG, JPEG, etc.)
/// 2. Resize to printer width (512px), maintaining aspect ratio
/// 3. Convert to grayscale
/// 4. Adaptive contrast + gamma based on image brightness
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

    // Full thermal preprocessing pipeline (adaptive)
    thermal_pipeline(&mut gray);

    // Re-encode as PNG
    let dithered = DynamicImage::ImageLuma8(gray);
    let mut buf = std::io::Cursor::new(Vec::new());
    dithered
        .write_to(&mut buf, image::ImageFormat::Png)
        .map_err(|e| format!("PNG encode failed: {e}"))?;

    Ok(buf.into_inner())
}

/// Full thermal print preprocessing: auto-levels → adaptive contrast/gamma → sharpen → dither.
/// Call this on an already-resized `GrayImage` before encoding to PNG for escpos.
pub fn dither_for_thermal(img: &mut GrayImage) {
    thermal_pipeline(img);
}

/// Adaptive thermal pipeline. Measures brightness after auto-levels to choose
/// contrast and gamma parameters — dark images get gentler contrast and more
/// aggressive gamma lift so shadow detail survives dithering.
fn thermal_pipeline(img: &mut GrayImage) {
    auto_levels(img);

    let mean = mean_brightness(img);
    let (contrast, gamma) = if mean < 90 {
        // Dark image: go easy on contrast, aggressively lift midtones
        (1.1_f32, 1.5_f32)
    } else if mean < 130 {
        // Medium image: moderate boost
        (1.25, 1.3)
    } else {
        // Normal/bright image: original behavior
        (1.4, 1.15)
    };
    tracing::debug!(
        "Thermal pipeline: mean brightness={mean}, contrast={contrast}, gamma={gamma}"
    );

    apply_contrast(img, contrast);
    apply_gamma(img, gamma);
    unsharp_mask(img, 0.5);
    floyd_steinberg_dither(img);
}

/// Average pixel brightness (0–255).
fn mean_brightness(img: &GrayImage) -> u8 {
    let total: u64 = img.pixels().map(|p| p[0] as u64).sum();
    let count = (img.width() as u64) * (img.height() as u64);
    if count == 0 {
        return 128;
    }
    (total / count) as u8
}

/// Stretch histogram so 2nd–98th percentile maps to 0–255.
/// Expands tonal range for images that don't use the full brightness spectrum.
fn auto_levels(img: &mut GrayImage) {
    let mut histogram = [0u32; 256];
    for pixel in img.pixels() {
        histogram[pixel[0] as usize] += 1;
    }

    let total = img.width() * img.height();
    let low_cutoff = (total as f32 * 0.02) as u32;
    let high_cutoff = (total as f32 * 0.98) as u32;

    // Find 2nd percentile
    let mut cumulative = 0u32;
    let mut low = 0u8;
    for (i, &count) in histogram.iter().enumerate() {
        cumulative += count;
        if cumulative >= low_cutoff {
            low = i as u8;
            break;
        }
    }

    // Find 98th percentile
    cumulative = 0;
    let mut high = 255u8;
    for (i, &count) in histogram.iter().enumerate() {
        cumulative += count;
        if cumulative >= high_cutoff {
            high = i as u8;
            break;
        }
    }

    if high <= low {
        return; // Image is essentially flat, nothing to stretch
    }

    // Build LUT to stretch [low, high] → [0, 255]
    let range = (high - low) as f32;
    let lut: Vec<u8> = (0..=255u16)
        .map(|v| {
            if v <= low as u16 {
                0
            } else if v >= high as u16 {
                255
            } else {
                ((v as f32 - low as f32) / range * 255.0).round() as u8
            }
        })
        .collect();

    for pixel in img.pixels_mut() {
        pixel[0] = lut[pixel[0] as usize];
    }
}

/// Apply contrast adjustment. factor > 1.0 increases contrast, < 1.0 decreases.
/// Pivots around 128 (midgray).
fn apply_contrast(img: &mut GrayImage, factor: f32) {
    let lut: Vec<u8> = (0..=255u16)
        .map(|v| {
            let centered = v as f32 - 128.0;
            let adjusted = centered * factor + 128.0;
            adjusted.round().clamp(0.0, 255.0) as u8
        })
        .collect();

    for pixel in img.pixels_mut() {
        pixel[0] = lut[pixel[0] as usize];
    }
}

/// Simple 3x3 unsharp mask to preserve edges before dithering.
/// amount controls sharpening strength (0.0 = none, 1.0 = full).
fn unsharp_mask(img: &mut GrayImage, amount: f32) {
    let width = img.width() as i32;
    let height = img.height() as i32;
    let src: Vec<u8> = img.pixels().map(|p| p[0]).collect();

    for y in 1..height - 1 {
        for x in 1..width - 1 {
            // 3x3 box blur for the pixel neighborhood
            let mut sum = 0i32;
            for dy in -1..=1i32 {
                for dx in -1..=1i32 {
                    sum += src[((y + dy) * width + (x + dx)) as usize] as i32;
                }
            }
            let blurred = sum / 9;
            let original = src[(y * width + x) as usize] as i32;
            let sharpened = original as f32 + (original - blurred) as f32 * amount;
            img.get_pixel_mut(x as u32, y as u32)[0] = sharpened.round().clamp(0.0, 255.0) as u8;
        }
    }
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
        apply_gamma(&mut img, 1.15);
        // Gamma > 1.0 should lighten midtones (128 → higher value)
        assert!(img.get_pixel(0, 0)[0] > 128);
    }

    #[test]
    fn gamma_preserves_extremes() {
        let mut img = GrayImage::new(2, 1);
        img.put_pixel(0, 0, image::Luma([0u8]));
        img.put_pixel(1, 0, image::Luma([255u8]));
        apply_gamma(&mut img, 1.15);
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
    fn mean_brightness_correct() {
        let img = GrayImage::from_pixel(10, 10, image::Luma([100u8]));
        assert_eq!(mean_brightness(&img), 100);
    }

    #[test]
    fn dark_image_gets_more_gamma_lift() {
        // Dark image (mean 60 after auto-levels — stays 60 since it's uniform)
        let mut dark = GrayImage::from_pixel(100, 100, image::Luma([60u8]));
        // Bright image (mean 180)
        let mut bright = GrayImage::from_pixel(100, 100, image::Luma([180u8]));

        // Apply just the contrast+gamma portion (skip auto_levels since uniform
        // images return unchanged, and skip dither for comparison)
        let dark_mean = mean_brightness(&dark);
        let bright_mean = mean_brightness(&bright);

        let (dc, dg) = if dark_mean < 90 { (1.1_f32, 1.5_f32) } else { (1.4, 1.15) };
        let (bc, bg) = if bright_mean < 90 { (1.1_f32, 1.5_f32) } else { (1.4, 1.15) };

        apply_contrast(&mut dark, dc);
        apply_gamma(&mut dark, dg);
        apply_contrast(&mut bright, bc);
        apply_gamma(&mut bright, bg);

        // Dark image pixel should have been lifted more aggressively
        // With (1.1, 1.5): 60 → contrast → ~53 → gamma 1.5 → ~100
        // With (1.4, 1.15): 60 → contrast → ~33 → gamma 1.15 → ~44
        // So the dark-adapted path produces brighter output
        assert!(
            dark.get_pixel(0, 0)[0] > 80,
            "Dark image pixel should be lifted above 80, got {}",
            dark.get_pixel(0, 0)[0]
        );
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
