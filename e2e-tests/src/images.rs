//! Comparing a rendered image against a checked-in reference.

use std::fs;
use std::path::Path;

use image::{DynamicImage, ImageFormat};
use image_compare::rgba_hybrid_compare;

/// How alike two renders of the same image have to be.
///
/// Anti-aliasing differs between the graphics stacks the tests run on, and JPEG loses more of the
/// image than PNG does.
const MIN_SIMILARITY: f64 = 0.99;
const MIN_JPEG_SIMILARITY: f64 = 0.95;

/// Assert that `body` renders what the reference image at `path` holds.
///
/// A reference that does not exist yet is written from `body`, so a new case is blessed by running
/// it and committing the file.
/// On CI it is a failure instead, keeping a render nothing has ever looked at from becoming its own
/// expectation there.
///
/// # Panics
/// If `body` is not a decodable image, if it differs from the reference, or if the reference is
/// missing on CI.
pub fn assert_image_matches(path: impl AsRef<Path>, body: &[u8]) {
    let (path, rendered) = (path.as_ref(), decode(body));
    if !path.is_file() {
        assert!(
            std::env::var_os("CI").is_none(),
            "no reference image at {}; run this test off CI to render one, then commit it",
            path.display()
        );
        let parent = path.parent().expect("a reference image is inside a folder");
        fs::create_dir_all(parent)
            .unwrap_or_else(|e| panic!("failed to create {}: {e}", parent.display()));
        fs::write(path, body).unwrap_or_else(|e| panic!("failed to write {}: {e}", path.display()));
        return;
    }
    let reference =
        decode(&fs::read(path).unwrap_or_else(|e| {
            panic!("failed to read the reference image {}: {e}", path.display())
        }));
    let minimum = if rendered.format == ImageFormat::Jpeg {
        MIN_JPEG_SIMILARITY
    } else {
        MIN_SIMILARITY
    };
    let similarity = similarity(&reference.image, &rendered.image);
    assert!(
        similarity >= minimum,
        "{} differs from what was rendered: similarity {similarity:.4} < {minimum}; \
         delete the reference and re-run to accept the new render",
        path.display()
    );
}

/// Assert that `a` and `b` are renders of the same image, e.g. two ways of framing one camera.
///
/// # Panics
/// If either body is not a decodable image, or if they differ.
pub fn assert_images_alike(a: &[u8], b: &[u8]) {
    let similarity = similarity(&decode(a).image, &decode(b).image);
    assert!(
        similarity >= MIN_SIMILARITY,
        "the images differ: similarity {similarity:.4} < {MIN_SIMILARITY}"
    );
}

/// Assert that `a` and `b` are renders of different images, e.g. that a camera option was applied.
///
/// # Panics
/// If either body is not a decodable image, or if they are alike.
pub fn assert_images_differ(a: &[u8], b: &[u8]) {
    let similarity = similarity(&decode(a).image, &decode(b).image);
    assert!(
        similarity < MIN_SIMILARITY,
        "the images are the same render: similarity {similarity:.4}"
    );
}

/// A decoded image, and the format its bytes were in.
struct Decoded {
    image: DynamicImage,
    format: ImageFormat,
}

fn decode(body: &[u8]) -> Decoded {
    let reader = image::ImageReader::new(std::io::Cursor::new(body))
        .with_guessed_format()
        .expect("reading from memory cannot fail");
    let format = reader.format().expect("the body is not a raster image");
    Decoded {
        image: reader.decode().expect("the body is not a decodable image"),
        format,
    }
}

/// How alike two images are, from 0.0 to 1.0.
///
/// # Panics
/// If the images are of different sizes.
fn similarity(reference: &DynamicImage, rendered: &DynamicImage) -> f64 {
    let (reference, rendered) = (reference.to_rgba8(), rendered.to_rgba8());
    assert_eq!(
        (rendered.width(), rendered.height()),
        (reference.width(), reference.height()),
        "the images are of different sizes"
    );
    rgba_hybrid_compare(&reference, &rendered)
        .expect("image_compare succeeds on equal-sized images")
        .score
}
