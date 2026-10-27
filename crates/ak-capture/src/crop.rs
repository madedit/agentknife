//! Cutting a rectangle out of a PNG (safe code; the `png` crate).

use std::io::Cursor;

use png::{BitDepth, ColorType, Transformations};

/// A rectangle in an image's pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// A PNG's width and height.
pub fn size(png: &[u8]) -> Result<(u32, u32), String> {
    let reader = png::Decoder::new(Cursor::new(png))
        .read_info()
        .map_err(|e| format!("not a PNG: {e}"))?;
    Ok(reader.info().size())
}

/// `rect` of a PNG, clipped to the image, as a new PNG (8-bit RGBA).
pub fn crop(png: &[u8], rect: PixelRect) -> Result<Vec<u8>, String> {
    let mut decoder = png::Decoder::new(Cursor::new(png));
    decoder.set_transformations(Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|e| format!("not a PNG: {e}"))?;
    let size = reader
        .output_buffer_size()
        .ok_or("the image is too large")?;
    let mut pixels = vec![0; size];
    let frame = reader
        .next_frame(&mut pixels)
        .map_err(|e| format!("could not read the image: {e}"))?;
    let channels = match (frame.color_type, frame.bit_depth) {
        (ColorType::Rgba, BitDepth::Eight) => 4,
        (ColorType::Rgb, BitDepth::Eight) => 3,
        (ColorType::GrayscaleAlpha, BitDepth::Eight) => 2,
        (ColorType::Grayscale, BitDepth::Eight) => 1,
        (c, d) => return Err(format!("unexpected image format {c:?} {d:?}")),
    };
    let x0 = rect.x.min(frame.width);
    let y0 = rect.y.min(frame.height);
    let x1 = rect.x.saturating_add(rect.width).min(frame.width);
    let y1 = rect.y.saturating_add(rect.height).min(frame.height);
    if x1 <= x0 || y1 <= y0 {
        return Err("the area is outside the image".into());
    }
    let (w, h) = (x1 - x0, y1 - y0);
    let mut out = Vec::with_capacity(w as usize * h as usize * 4);
    for y in y0..y1 {
        let row = &pixels[y as usize * frame.line_size..][..frame.line_size];
        for x in x0..x1 {
            let p = &row[x as usize * channels..][..channels];
            out.extend_from_slice(&match channels {
                4 => [p[0], p[1], p[2], p[3]],
                3 => [p[0], p[1], p[2], 255],
                2 => [p[0], p[0], p[0], p[1]],
                _ => [p[0], p[0], p[0], 255],
            });
        }
    }
    let mut png_out = Vec::new();
    let mut encoder = png::Encoder::new(&mut png_out, w, h);
    encoder.set_color(ColorType::Rgba);
    encoder.set_depth(BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|e| format!("could not write the image: {e}"))?;
    writer
        .write_image_data(&out)
        .map_err(|e| format!("could not write the image: {e}"))?;
    writer
        .finish()
        .map_err(|e| format!("could not write the image: {e}"))?;
    Ok(png_out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `w`×`h` RGB image whose pixel (x, y) is (x, y, 7).
    fn image(w: u32, h: u32) -> Vec<u8> {
        let mut data = Vec::new();
        for y in 0..h {
            for x in 0..w {
                data.extend_from_slice(&[x as u8, y as u8, 7]);
            }
        }
        let mut out = Vec::new();
        let mut e = png::Encoder::new(&mut out, w, h);
        e.set_color(ColorType::Rgb);
        e.set_depth(BitDepth::Eight);
        let mut writer = e.write_header().unwrap();
        writer.write_image_data(&data).unwrap();
        writer.finish().unwrap();
        out
    }

    fn pixels(png: &[u8]) -> (u32, u32, Vec<u8>) {
        let mut reader = png::Decoder::new(Cursor::new(png)).read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buf).unwrap();
        assert_eq!(info.color_type, ColorType::Rgba);
        (info.width, info.height, buf)
    }

    #[test]
    fn crops_the_rectangle_and_clips_it_to_the_image() {
        let png = image(10, 8);
        assert_eq!(size(&png).unwrap(), (10, 8));
        let cut = crop(
            &png,
            PixelRect {
                x: 2,
                y: 3,
                width: 4,
                height: 2,
            },
        )
        .unwrap();
        let (w, h, px) = pixels(&cut);
        assert_eq!((w, h), (4, 2));
        assert_eq!(&px[..4], &[2, 3, 7, 255], "top left is (2, 3)");
        assert_eq!(
            &px[px.len() - 4..],
            &[5, 4, 7, 255],
            "bottom right is (5, 4)"
        );

        let clipped = crop(
            &png,
            PixelRect {
                x: 8,
                y: 6,
                width: 50,
                height: 50,
            },
        )
        .unwrap();
        assert_eq!(size(&clipped).unwrap(), (2, 2));
        let outside = PixelRect {
            x: 10,
            y: 0,
            width: 5,
            height: 5,
        };
        assert!(crop(&png, outside).is_err());
        assert!(size(b"not a png").is_err());
    }
}
