//! The business logo: any common picture format, shrunk to fit and stored
//! as PNG so receipts stay small and the sidebar draws it crisply.

use std::io::Cursor;

use image::{ImageFormat, imageops::FilterType};

/// Longest side of the stored logo, in pixels.
const MAX_SIDE: u32 = 512;
/// Largest file accepted for upload.
const MAX_UPLOAD: usize = 20 * 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub enum LogoError {
    TooLarge,
    Unreadable,
}

/// Decodes an uploaded picture and returns it as a PNG no larger than
/// `MAX_SIDE` on its longest side. Transparency is kept.
pub fn prepare(bytes: &[u8]) -> Result<Vec<u8>, LogoError> {
    if bytes.len() > MAX_UPLOAD {
        return Err(LogoError::TooLarge);
    }
    let img = image::load_from_memory(bytes).map_err(|_| LogoError::Unreadable)?;
    let img = if img.width() > MAX_SIDE || img.height() > MAX_SIDE { img.resize(MAX_SIDE, MAX_SIDE, FilterType::Lanczos3) } else { img };
    let mut out = Cursor::new(Vec::new());
    img.to_rgba8().write_to(&mut out, ImageFormat::Png).map_err(|_| LogoError::Unreadable)?;
    Ok(out.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba([30, 90, 200, 255]));
        let mut out = Cursor::new(Vec::new());
        img.write_to(&mut out, ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn shrinks_large_pictures_and_keeps_the_shape() {
        let out = prepare(&png(2000, 1000)).unwrap();
        let img = image::load_from_memory(&out).unwrap();
        assert_eq!((img.width(), img.height()), (512, 256));
    }

    #[test]
    fn keeps_small_pictures_and_rejects_other_files() {
        let img = image::load_from_memory(&prepare(&png(64, 64)).unwrap()).unwrap();
        assert_eq!(img.width(), 64);
        assert_eq!(prepare(b"not a picture"), Err(LogoError::Unreadable));
    }
}
