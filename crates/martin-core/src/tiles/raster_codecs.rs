//! Glue that teaches the `image` crate about raster codecs it doesn't support natively.

/// Makes `image::load_from_memory` (and `ImageReader::with_guessed_format`) understand JPEG XL.
pub fn ensure_jxl_decoding_hook() {
    static REGISTERED: std::sync::Once = std::sync::Once::new();
    REGISTERED.call_once(|| {
        jxl_oxide::integration::register_image_decoding_hook();
    });
}
