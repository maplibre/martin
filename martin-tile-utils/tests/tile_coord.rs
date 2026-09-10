use martin_tile_utils::{MAX_ZOOM, TileCoord};

#[test]
fn tile_coord_zoom_range() {
    for z in 0..=MAX_ZOOM {
        assert!(TileCoord::is_possible_on_zoom_level(z, 0, 0));
        assert_eq!(
            TileCoord::new_checked(z, 0, 0),
            Some(TileCoord::new_unchecked(z, 0, 0))
        );
    }
    assert!(!TileCoord::is_possible_on_zoom_level(MAX_ZOOM + 1, 0, 0));
    assert_eq!(TileCoord::new_checked(MAX_ZOOM + 1, 0, 0), None);
}

#[test]
fn tile_coord_new_checked_xy_for_zoom() {
    assert!(TileCoord::is_possible_on_zoom_level(5, 0, 0));
    assert_eq!(
        TileCoord::new_checked(5, 0, 0),
        Some(TileCoord::new_unchecked(5, 0, 0))
    );
    assert!(TileCoord::is_possible_on_zoom_level(5, 31, 31));
    assert_eq!(
        TileCoord::new_checked(5, 31, 31),
        Some(TileCoord::new_unchecked(5, 31, 31))
    );
    assert!(!TileCoord::is_possible_on_zoom_level(5, 31, 32));
    assert_eq!(TileCoord::new_checked(5, 31, 32), None);
    assert!(!TileCoord::is_possible_on_zoom_level(5, 32, 31));
    assert_eq!(TileCoord::new_checked(5, 32, 31), None);
}

#[test]
fn tile_coord_is_possible_on_the_last_zoom_level() {
    let last = (1_u32 << MAX_ZOOM) - 1;
    assert!(TileCoord::is_possible_on_zoom_level(MAX_ZOOM, last, last));
    assert!(!TileCoord::is_possible_on_zoom_level(
        MAX_ZOOM,
        last,
        last + 1
    ));
}

#[test]
fn packed_id_leading_zeros_are_the_zoom_level() {
    for z in 0..=MAX_ZOOM {
        let last = (1_u32 << z) - 1;
        for (x, y) in [(0, 0), (last, 0), (0, last), (last, last)] {
            let coord = TileCoord::new_unchecked(z, x, y);
            assert_eq!((coord.z(), coord.x(), coord.y()), (z, x, y));
        }
    }
}

#[test]
fn xyz_format() {
    let xyz = TileCoord::new_unchecked(4, 2, 3);
    assert_eq!(format!("{xyz}"), "4,2,3");
    assert_eq!(format!("{xyz:#}"), "4/2/3");
}
