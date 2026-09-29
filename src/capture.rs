//! Image file output shared by frame captures.

use crate::error::{Error, Result};
use std::path::Path;

/// Write an RGBA image as PNG or JPEG depending on the path's extension.
pub fn save_rgba(rgba: image::RgbaImage, path: &Path) -> Result<()> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png")
        .to_ascii_lowercase();
    let result = if ext == "jpg" || ext == "jpeg" {
        image::DynamicImage::ImageRgba8(rgba)
            .to_rgb8()
            .save_with_format(path, image::ImageFormat::Jpeg)
    } else {
        rgba.save_with_format(path, image::ImageFormat::Png)
    };
    result.map_err(|e| {
        Error::Io(std::io::Error::other(format!(
            "write {}: {e}",
            path.display()
        )))
    })
}

/// Build an image from tightly packed RGBA rows read back from a framebuffer that libmpv
/// rendered without `FLIP_Y`, which already places the first row at the top.
#[cfg(not(windows))]
pub fn from_gl_pixels(w: u32, h: u32, rows: &[u8]) -> Option<image::RgbaImage> {
    let stride = w as usize * 4;
    if rows.len() < stride * h as usize {
        return None;
    }
    image::RgbaImage::from_raw(w, h, rows[..stride * h as usize].to_vec())
}

/// Rewrite an image file so its width is at most `width`, keeping the aspect ratio.
pub fn shrink(path: &Path, width: u32) -> Result<()> {
    let img = image::open(path).map_err(|e| {
        Error::Io(std::io::Error::other(format!(
            "read {}: {e}",
            path.display()
        )))
    })?;
    if img.width() <= width {
        return Ok(());
    }
    let small = img.thumbnail(width, u32::MAX);
    save_rgba(small.to_rgba8(), path)
}
