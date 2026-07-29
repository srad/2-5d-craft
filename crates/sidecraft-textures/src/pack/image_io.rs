use crate::{PackError, PackImage};
use image::{ImageBuffer, RgbaImage};
use std::{fs, path::Path};

const MAX_PNG_BYTES: u64 = 4 * 1024 * 1024;

pub(super) fn decode_png(path: &Path, width: u32, height: u32) -> Result<PackImage, PackError> {
    if fs::metadata(path)?.len() > MAX_PNG_BYTES {
        return Err(PackError::Invalid(format!(
            "PNG exceeds the 4 MiB decode limit: {}",
            path.display()
        )));
    }
    let dimensions = image::image_dimensions(path)?;
    if dimensions != (width, height) {
        return Err(PackError::Invalid(format!(
            "{} is {}x{}, expected {width}x{height}",
            path.display(),
            dimensions.0,
            dimensions.1
        )));
    }
    let decoded = image::open(path)?.into_rgba8();
    PackImage::new(width, height, decoded.into_raw())
}

pub(super) fn write_png(path: &Path, image: &PackImage) -> Result<(), PackError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let buffer: RgbaImage = ImageBuffer::from_raw(image.width, image.height, image.pixels.clone())
        .ok_or_else(|| PackError::Invalid("invalid RGBA image".into()))?;
    buffer.save_with_format(path, image::ImageFormat::Png)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_dimensions_are_enforced_before_decode() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("image.png");
        write_png(&path, &PackImage::solid(16, 16, [1, 2, 3, 255])).unwrap();
        assert!(decode_png(&path, 16, 16).is_ok());
        assert!(decode_png(&path, 32, 32).is_err());
    }
}
