//! Pictures people upload: the business logo and client photos. Any common
//! format is accepted, turned upright, shrunk to fit and stored re-encoded,
//! which also drops hidden metadata such as the camera and the location.

use std::io::Cursor;

use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, ImageDecoder as _, ImageFormat, ImageReader, imageops::FilterType};

use crate::i18n::t;

/// Largest file accepted for upload.
const MAX_UPLOAD: usize = 20 * 1024 * 1024;
/// Longest side of the stored logo, in pixels.
const LOGO_SIDE: u32 = 512;
/// Longest side of a stored client photo: an ID card stays legible.
const PHOTO_SIDE: u32 = 2000;
const PHOTO_QUALITY: u8 = 85;

#[derive(Debug, PartialEq, Eq)]
pub enum PictureError {
    TooLarge,
    Unreadable,
}

impl PictureError {
    pub fn message(&self) -> &'static str {
        match self {
            PictureError::TooLarge => t("The picture is too large (20 MB at most)", "La imagen es demasiado grande (máximo 20 MB)"),
            PictureError::Unreadable => t(
                "This file isn't a picture the app can read. Use a PNG, JPG, GIF, BMP or WebP image.",
                "La app no puede leer este archivo. Usa una imagen PNG, JPG, GIF, BMP o WebP.",
            ),
        }
    }
}

/// Decodes an upload and turns it upright when the camera recorded it
/// sideways.
fn decode(bytes: &[u8]) -> Result<DynamicImage, PictureError> {
    if bytes.len() > MAX_UPLOAD {
        return Err(PictureError::TooLarge);
    }
    let reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format().map_err(|_| PictureError::Unreadable)?;
    let mut decoder = reader.into_decoder().map_err(|_| PictureError::Unreadable)?;
    let orientation = decoder.orientation().ok();
    let mut img = DynamicImage::from_decoder(decoder).map_err(|_| PictureError::Unreadable)?;
    if let Some(o) = orientation {
        img.apply_orientation(o);
    }
    Ok(img)
}

fn fit(img: DynamicImage, side: u32) -> DynamicImage {
    if img.width() > side || img.height() > side { img.resize(side, side, FilterType::Lanczos3) } else { img }
}

/// The logo as a PNG no larger than `LOGO_SIDE` on its longest side.
/// Transparency is kept.
pub fn logo_png(bytes: &[u8]) -> Result<Vec<u8>, PictureError> {
    let img = fit(decode(bytes)?, LOGO_SIDE);
    let mut out = Cursor::new(Vec::new());
    img.to_rgba8().write_to(&mut out, ImageFormat::Png).map_err(|_| PictureError::Unreadable)?;
    Ok(out.into_inner())
}

/// A client photo ready to store: a JPEG no larger than `PHOTO_SIDE` on its
/// longest side, with transparent areas made white. Its bytes never appear
/// in debug output or logs.
#[derive(Clone, PartialEq, Eq)]
pub struct Photo(Vec<u8>);

impl Photo {
    pub fn from_upload(bytes: &[u8]) -> Result<Self, PictureError> {
        let img = fit(decode(bytes)?, PHOTO_SIDE);
        let mut rgb = image::RgbImage::new(img.width(), img.height());
        for (x, y, px) in img.to_rgba8().enumerate_pixels() {
            let [r, g, b, a] = px.0;
            let over_white = |c: u8| ((c as u16 * a as u16 + 255 * (255 - a as u16)) / 255) as u8;
            rgb.put_pixel(x, y, image::Rgb([over_white(r), over_white(g), over_white(b)]));
        }
        let mut out = Vec::new();
        JpegEncoder::new_with_quality(&mut out, PHOTO_QUALITY).encode_image(&rgb).map_err(|_| PictureError::Unreadable)?;
        Ok(Self(out))
    }

    pub fn jpeg(&self) -> &[u8] {
        &self.0
    }
}

impl std::fmt::Debug for Photo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Photo({} bytes)", self.0.len())
    }
}

/// What saving a client does to their photo.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum PhotoChange {
    #[default]
    Keep,
    Set(Photo),
    Remove,
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn png(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba([30, 90, 200, 255]));
        let mut out = Cursor::new(Vec::new());
        img.write_to(&mut out, ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn shrinks_large_logos_and_keeps_the_shape() {
        let out = logo_png(&png(2000, 1000)).unwrap();
        let img = image::load_from_memory(&out).unwrap();
        assert_eq!((img.width(), img.height()), (512, 256));
    }

    #[test]
    fn keeps_small_pictures_and_rejects_other_files() {
        let img = image::load_from_memory(&logo_png(&png(64, 64)).unwrap()).unwrap();
        assert_eq!(img.width(), 64);
        assert_eq!(logo_png(b"not a picture"), Err(PictureError::Unreadable));
        assert_eq!(Photo::from_upload(b"not a picture"), Err(PictureError::Unreadable));
        assert_eq!(Photo::from_upload(&vec![0; MAX_UPLOAD + 1]), Err(PictureError::TooLarge));
    }

    #[test]
    fn photos_become_jpegs_that_fit_and_never_print_their_bytes() {
        let photo = Photo::from_upload(&png(4000, 2500)).unwrap();
        assert_eq!(image::guess_format(photo.jpeg()).unwrap(), ImageFormat::Jpeg);
        let img = image::load_from_memory(photo.jpeg()).unwrap();
        assert_eq!((img.width(), img.height()), (2000, 1250));
        let shown = format!("{:?}", PhotoChange::Set(photo.clone()));
        assert_eq!(shown, format!("Set(Photo({} bytes))", photo.jpeg().len()));
        // A JPEG upload is read too.
        assert!(Photo::from_upload(photo.jpeg()).is_ok());
    }
}
