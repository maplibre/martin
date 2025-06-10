## COG Image Representation

* COG file is an image container representing a tile grid
* A COG may have multiple images, also called subfiles, each indexed with an Image File Directory number - `IFD`. (TODO: link to TIFF 6.0 spec)
* A COG must have at least one image.
* The first image (IFD=0) must be a full resolution image, e.g., the one with the highest resolution.
* Each image may also have a mask, which is also indexed with an IFD. The mask is used to identify which pixel is nodata. We do not support masks yet. (TODO: link)
* While uncommon, COG tile grid might be different from the common ones like Web Mercator.

### COG structure example

Here is an example of a tile grid for a COG file with five images and five masks.

| ifd | image index | resolution | zoom |
|-----|-------------|------------|------|
| 0   | 0           | 20         | 4    |
| 2   | 1           | 40         | 3    |
| 4   | 2           | 80         | 2    |
| 6   | 3           | 160        | 1    |
| 8   | 4           | 320        | 0    |

### Tile grid code representation

```rust, ignore
let images = vec![ image_0, image_1, image_2, image_3, image_4 ];
let minzoom = 0;
let zoom_of_image = image_count - 1 - idx_in_vec;  # TODO: what is this?
let maxzoom = images.len() - 1;
```
