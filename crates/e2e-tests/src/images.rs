//! Comparing a rendered image against a checked-in reference.

use std::fs;
use std::path::Path;

use blazediff::{DiffOptions, DiffResult, Image, Metric, WindowHit};
use image::{DynamicImage, RgbaImage};

/// How far apart two pixels may be, as pixelmatch 8's `OKLab` `HyAB` distance (1 is black against
/// white), and still count as the same.
const PIXEL_THRESHOLD: f64 = 0.1;
/// [`PIXEL_THRESHOLD`] for JPEG, whose compression moves colors further.
const JPEG_PIXEL_THRESHOLD: f64 = 0.2;
/// The side of the square windows that [`MAX_DIFFERING_PER_WINDOW`] counts in.
const WINDOW: u32 = 16;
/// How many pixels of any [`WINDOW`]-sided square may differ beyond anti-aliasing, as graphics
/// stacks scatter a few one-pixel shifts over an image while a rendering bug clusters them.
const MAX_DIFFERING_PER_WINDOW: u32 = 20;

/// Assert that `body` renders what the reference image at `path` holds.
///
/// A reference that does not exist yet is written from `body`, so a new case is blessed by running
/// it and committing the file.
/// On CI it is a failure instead, keeping a render nothing has ever looked at from becoming its own
/// expectation there.
/// A render that differs is written next to its reference as `<name>.actual.<ext>`, and the pixels
/// that differ as `<name>.actual.diff.png`, for inspection.
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
        fs::create_dir_all(parent).unwrap_or_else(|e| panic!("failed to create {}: {e}", parent.display()));
        fs::write(path, body).unwrap_or_else(|e| panic!("failed to write {}: {e}", path.display()));
        return;
    }
    let reference = decode(
        &fs::read(path).unwrap_or_else(|e| panic!("failed to read the reference image {}: {e}", path.display())),
    );
    let difference = compare(&reference.image, &rendered.image, rendered.is_jpeg);
    if !difference.matches() {
        let extension = path.extension().expect("a reference image has an extension");
        let actual = path.with_extension(format!("actual.{}", extension.display()));
        fs::write(&actual, body).unwrap_or_else(|e| panic!("failed to write {}: {e}", actual.display()));
        let diff = path.with_extension("actual.diff.png");
        difference
            .image
            .save(&diff)
            .unwrap_or_else(|e| panic!("failed to write {}: {e}", diff.display()));
    }
    assert!(
        difference.matches(),
        "{} differs from what was rendered: {difference}; \
         delete the reference and re-run to accept the new render",
        path.display()
    );
}

/// Assert that `a` and `b` are renders of the same image, e.g. two ways of framing one camera.
///
/// # Panics
/// If either body is not a decodable image, or if they differ.
pub fn assert_images_alike(a: &[u8], b: &[u8]) {
    let (a, b) = (decode(a), decode(b));
    let difference = compare(&a.image, &b.image, a.is_jpeg || b.is_jpeg);
    assert!(difference.matches(), "the images differ: {difference}");
}

/// Assert that `a` and `b` are renders of different images, e.g. that a camera option was applied.
///
/// # Panics
/// If either body is not a decodable image, or if they are alike.
pub fn assert_images_differ(a: &[u8], b: &[u8]) {
    let (a, b) = (decode(a), decode(b));
    let difference = compare(&a.image, &b.image, a.is_jpeg || b.is_jpeg);
    assert!(!difference.matches(), "the images are the same render: {difference}");
}

/// A decoded image, and whether its bytes were JPEG.
struct Decoded {
    image: DynamicImage,
    is_jpeg: bool,
}

fn decode(body: &[u8]) -> Decoded {
    crate::ensure_jxl_decoding_hook();
    // Checked directly rather than through `ImageReader::format()`: that maps a guessed format
    // back to `image::ImageFormat` by extension, which has no JPEG XL entry even once the hook
    // above lets `image` decode it.
    let is_jpeg = body.starts_with(b"\xFF\xD8\xFF");
    let reader = image::ImageReader::new(std::io::Cursor::new(body))
        .with_guessed_format()
        .expect("reading from memory cannot fail");
    Decoded {
        image: reader.decode().expect("the body is not a decodable image"),
        is_jpeg,
    }
}

/// How many pixels of two equal-sized images differ, where they cluster most, and an image marking
/// them.
struct Difference {
    /// Pixels that differ beyond the threshold, leaving out anti-aliasing.
    differing: u32,
    /// Pixels in each image.
    total: u32,
    /// The [`WINDOW`]-sided square with the most differing pixels.
    busiest: WindowHit,
    /// The differing pixels in red over a faded copy of the first image.
    image: RgbaImage,
}

impl Difference {
    /// Whether the images are close enough to count as the same render.
    fn matches(&self) -> bool {
        self.busiest.count <= MAX_DIFFERING_PER_WINDOW
    }
}

impl std::fmt::Display for Difference {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let WindowHit { x, y, count } = self.busiest;
        write!(
            f,
            "{count} pixels differ in the {WINDOW}x{WINDOW} square at ({x}, {y}), at most \
             {MAX_DIFFERING_PER_WINDOW} may ({} of {} pixels differ in all)",
            self.differing, self.total
        )
    }
}

/// Compare `a` and `b`, each composited over white, as JPEG if either was one.
///
/// # Panics
/// If the images are of different sizes.
fn compare(a: &DynamicImage, b: &DynamicImage, is_jpeg: bool) -> Difference {
    let (a, b) = (over_white(a), over_white(b));
    assert_eq!(
        (a.width, a.height),
        (b.width, b.height),
        "the images are of different sizes"
    );
    let mut output = Image::new(a.width, a.height);
    let options = DiffOptions {
        threshold: if is_jpeg { JPEG_PIXEL_THRESHOLD } else { PIXEL_THRESHOLD },
        metric: Metric::Oklab,
        window: Some(WINDOW),
        ..DiffOptions::default()
    };
    let result = blazediff::diff(&a, &b, Some(&mut output), &options).expect("the buffers hold every pixel");
    let DiffResult::Compared {
        count,
        window: Some(busiest),
        ..
    } = result
    else {
        unreachable!("equal-sized images compare, with the window that was asked for")
    };
    Difference {
        differing: count,
        total: a.width * a.height,
        busiest,
        image: RgbaImage::from_raw(output.width, output.height, output.data)
            .expect("blazediff writes every pixel of its output"),
    }
}

/// `image` composited over opaque white, so a transparent pixel compares as white.
fn over_white(image: &DynamicImage) -> Image {
    let image = image.to_rgba8();
    let (width, height) = image.dimensions();
    let data = image
        .pixels()
        .flat_map(|pixel| {
            let [red, green, blue, alpha] = pixel.0;
            let blend = |channel: u8| {
                let blended = (u32::from(channel) * u32::from(alpha) + 255 * (255 - u32::from(alpha))) / 255;
                u8::try_from(blended).expect("a blend of two bytes fits in one")
            };
            [blend(red), blend(green), blend(blue), 255]
        })
        .collect();
    Image { data, width, height }
}
