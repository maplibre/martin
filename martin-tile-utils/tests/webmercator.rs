use approx::assert_relative_eq;
use martin_tile_utils::{
    EARTH_CIRCUMFERENCE, bbox_to_xyz, get_zoom_precision, tile_bbox, tile_index,
    webmercator_to_wgs84, wgs84_to_webmercator, xyz_to_bbox,
};
use rstest::rstest;

#[rstest]
#[case(-180.0, 85.0511, 0, (0,0))]
#[case(-180.0, 85.0511, 1, (0,0))]
#[case(-180.0, 85.0511, 2, (0,0))]
#[case(0.0, 0.0, 0, (0,0))]
#[case(0.0, 0.0, 1, (1,1))]
#[case(0.0, 0.0, 2, (2,2))]
#[case(0.0, 1.0, 0, (0,0))]
#[case(0.0, 1.0, 1, (1,0))]
#[case(0.0, 1.0, 2, (2,1))]
fn test_tile_colrow(
    #[case] lng: f64,
    #[case] lat: f64,
    #[case] zoom: u8,
    #[case] expected: (u32, u32),
) {
    assert_eq!(
        expected,
        tile_index(lng, lat, zoom),
        "{lng},{lat}@z{zoom} should be {expected:?}"
    );
}

#[rstest]
// you could easily get test cases from maptiler: https://www.maptiler.com/google-maps-coordinates-tile-bounds-projection/#4/-118.82/71.02
#[case(0, 0, 0, 0, 0, [-180.0,-85.051_128_779_806_6,180.0,85.051_128_779_806_6])]
#[case(1, 0, 0, 0, 0, [-180.0,0.0,0.0,85.051_128_779_806_6])]
#[case(5, 1, 1, 2, 2, [-168.75,81.093_213_852_608_37,-146.25,83.979_259_498_862_05])]
#[case(5, 1, 3, 2, 5, [-168.75,74.019_543_311_502_26,-146.25,81.093_213_852_608_37])]
fn test_xyz_to_bbox(
    #[case] zoom: u8,
    #[case] min_x: u32,
    #[case] min_y: u32,
    #[case] max_x: u32,
    #[case] max_y: u32,
    #[case] expected: [f64; 4],
) {
    let bbox = xyz_to_bbox(zoom, min_x, min_y, max_x, max_y);
    assert_relative_eq!(bbox[0], expected[0], epsilon = f64::EPSILON * 2.0);
    assert_relative_eq!(bbox[1], expected[1], epsilon = f64::EPSILON * 2.0);
    assert_relative_eq!(bbox[2], expected[2], epsilon = f64::EPSILON * 2.0);
    assert_relative_eq!(bbox[3], expected[3], epsilon = f64::EPSILON * 2.0);
}

#[rstest]
#[case(0, 0, 0, [-20_037_508.342_789_25, -20_037_508.342_789_25, 20_037_508.342_789_25, 20_037_508.342_789_25])]
#[case(1, 0, 0, [-20_037_508.342_789_25, 0.0, 0.0, 20_037_508.342_789_25])]
#[case(1, 1, 1, [0.0, -20_037_508.342_789_25, 20_037_508.342_789_25, 0.0])]
#[case(2, 0, 0, [-20_037_508.342_789_25, 10_018_754.171_394_625, -10_018_754.171_394_625, 20_037_508.342_789_25])]
#[case(2, 2, 2, [0.0, -10_018_754.171_394_625, 10_018_754.171_394_625, 0.0])]
fn test_tile_bbox(#[case] zoom: u8, #[case] x: u32, #[case] y: u32, #[case] expected: [f64; 4]) {
    let tile_length = EARTH_CIRCUMFERENCE / f64::from(1_u32 << zoom);
    let bbox = tile_bbox(x, y, tile_length);
    assert_relative_eq!(bbox[0], expected[0], epsilon = f64::EPSILON * 2.0);
    assert_relative_eq!(bbox[1], expected[1], epsilon = f64::EPSILON * 2.0);
    assert_relative_eq!(bbox[2], expected[2], epsilon = f64::EPSILON * 2.0);
    assert_relative_eq!(bbox[3], expected[3], epsilon = f64::EPSILON * 2.0);
    assert_relative_eq!(bbox[2] - bbox[0], tile_length, epsilon = f64::EPSILON * 2.0);
    assert_relative_eq!(bbox[3] - bbox[1], tile_length, epsilon = f64::EPSILON * 2.0);
}

#[rstest]
#[case(0, (0, 0, 0, 0))]
#[case(1, (0, 1, 0, 1))]
#[case(2, (0, 3, 0, 3))]
#[case(3, (0, 7, 0, 7))]
#[case(4, (0, 14, 1, 15))]
#[case(5, (0, 29, 2, 31))]
#[case(6, (0, 58, 5, 63))]
#[case(7, (0, 116, 11, 126))]
#[case(8, (0, 233, 23, 253))]
#[case(9, (0, 466, 47, 507))]
#[case(10, (1, 933, 94, 1_014))]
#[case(11, (3, 1_866, 188, 2_029))]
#[case(12, (6, 3_732, 377, 4_059))]
#[case(13, (12, 7_465, 755, 8_119))]
#[case(14, (25, 14_931, 1_510, 16_239))]
#[case(15, (51, 29_863, 3_020, 32_479))]
#[case(16, (102, 59_727, 6_041, 64_958))]
#[case(17, (204, 119_455, 12_083, 129_917))]
#[case(18, (409, 238_911, 24_166, 259_834))]
#[case(19, (819, 477_823, 48_332, 519_669))]
#[case(20, (1_638, 955_647, 96_665, 1_039_339))]
#[case(21, (3_276, 1_911_295, 193_331, 2_078_678))]
#[case(22, (6_553, 3_822_590, 386_662, 4_157_356))]
#[case(23, (13_107, 7_645_181, 773_324, 8_314_713))]
#[case(24, (26_214, 15_290_363, 1_546_649, 16_629_427))]
#[case(25, (52_428, 30_580_726, 3_093_299, 33_258_855))]
#[case(26, (104_857, 61_161_453, 6_186_598, 66_517_711))]
#[case(27, (209_715, 122_322_907, 12_373_196, 133_035_423))]
#[case(28, (419_430, 244_645_814, 24_746_393, 266_070_846))]
#[case(29, (838_860, 489_291_628, 49_492_787, 532_141_692))]
#[case(30, (1_677_721, 978_583_256, 98_985_574, 1_064_283_385))]
fn test_box_to_xyz(#[case] zoom: u8, #[case] expected_xyz: (u32, u32, u32, u32)) {
    let actual_xyz = bbox_to_xyz(
        -179.437_499_999_999_55,
        -84.769_878_779_806_56,
        -146.812_499_999_999_6,
        -81.374_463_852_608_33,
        zoom,
    );
    assert_eq!(
        actual_xyz, expected_xyz,
        "zoom {zoom} does not have the right xyz"
    );
}

#[rstest]
// test data via https://epsg.io/transform#s_srs=4326&t_srs=3857
#[case((0.0,0.0), (0.0,0.0))]
#[case((30.0,0.0), (3_339_584.723_798_207,0.0))]
#[case((-30.0,0.0), (-3_339_584.723_798_207,0.0))]
#[case((0.0,30.0), (0.0,3_503_549.843_504_375_3))]
#[case((0.0,-30.0), (0.0,-3_503_549.843_504_375_3))]
#[case((38.897_957,-77.036_560), (4_330_100.766_138_651, -13_872_207.775_755_845))] // white house
#[case((-180.0,-85.0), (-20_037_508.342_789_244, -19_971_868.880_408_566))]
#[case((180.0,85.0), (20_037_508.342_789_244, 19_971_868.880_408_566))]
#[case((0.026_949_458_523_585_632,0.080_848_348_740_973_67), (3000.0, 9000.0))]
fn test_coordinate_syste_conversion(#[case] wgs84: (f64, f64), #[case] webmercator: (f64, f64)) {
    // epsg produces the expected values with f32 precision, grrr..
    let epsilon = f64::from(f32::EPSILON);

    let actual_wgs84 = webmercator_to_wgs84(webmercator.0, webmercator.1);
    assert_relative_eq!(actual_wgs84.0, wgs84.0, epsilon = epsilon);
    assert_relative_eq!(actual_wgs84.1, wgs84.1, epsilon = epsilon);

    let actual_webmercator = wgs84_to_webmercator(wgs84.0, wgs84.1);
    assert_relative_eq!(actual_webmercator.0, webmercator.0, epsilon = epsilon);
    assert_relative_eq!(actual_webmercator.1, webmercator.1, epsilon = epsilon);
}

#[rstest]
#[case(0..11, 0)]
#[case(11..14, 1)]
#[case(14..17, 2)]
#[case(17..21, 3)]
#[case(21..24, 4)]
#[case(24..27, 5)]
#[case(27..30, 6)]
fn test_get_zoom_precision(#[case] zoom: std::ops::Range<u8>, #[case] expected_precision: usize) {
    for z in zoom {
        let actual_precision = get_zoom_precision(z);
        assert_eq!(
            actual_precision, expected_precision,
            "Zoom level {z} should have precision {expected_precision}, but was {actual_precision}"
        );
    }
}
