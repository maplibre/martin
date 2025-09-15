use geozero::mvt::{Command, CommandInteger, ParameterInteger};

pub fn encode_geom(geom: &geojson::Geometry) -> Vec<u32> {
    match &geom.value {
        geojson::Value::Point(items) => encode_point(items[0] as i32, items[1] as i32),
        geojson::Value::MultiPoint(items) => encode_multipoint(
            &items
                .iter()
                .map(|p| (p[0] as i32, p[1] as i32))
                .collect::<Vec<_>>(),
        ),
        geojson::Value::LineString(items) => encode_linestring(
            &items
                .iter()
                .map(|p| (p[0] as i32, p[1] as i32))
                .collect::<Vec<_>>(),
        ),
        geojson::Value::MultiLineString(items) => encode_multilinestring(
            &items
                .iter()
                .map(|e| {
                    e.iter()
                        .map(|p| (p[0] as i32, p[1] as i32))
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>(),
        ),
        geojson::Value::Polygon(items) => encode_polygon(
            &items
                .iter()
                .map(|e| {
                    e.iter()
                        .map(|p| (p[0] as i32, p[1] as i32))
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>(),
        ),
        geojson::Value::MultiPolygon(items) => encode_multipolygon(
            &items
                .iter()
                .map(|e| {
                    e.iter()
                        .map(|p| {
                            p.iter()
                                .map(|q| (q[0] as i32, q[1] as i32))
                                .collect::<Vec<_>>()
                        })
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>(),
        ),
        _ => panic!("Unsupported geometry type"),
    }
}
pub fn encode_point(x: i32, y: i32) -> Vec<u32> {
    let cmd = CommandInteger::from(Command::MoveTo, 1);
    let dx = ParameterInteger::from(x);
    let dy = ParameterInteger::from(y);
    vec![cmd, dx, dy]
}

pub fn encode_multipoint(points: &[(i32, i32)]) -> Vec<u32> {
    let count = points.len();
    let mut encoded = Vec::with_capacity(count * 2 + 1);
    encoded.push(CommandInteger::from(Command::MoveTo, count as u32));

    let mut cx = 0;
    let mut cy = 0;
    for point in points {
        let (x, y) = point;
        let dx = x - cx;
        let dy = y - cy;
        encoded.push(ParameterInteger::from(dx));
        encoded.push(ParameterInteger::from(dy));
        cx = *x;
        cy = *y;
    }
    encoded
}

pub fn encode_linestring(points: &[(i32, i32)]) -> Vec<u32> {
    assert!(!points.is_empty()); // TODO: preferably >= 2

    let mut encoded = Vec::new();
    let cmd0 = CommandInteger::from(Command::MoveTo, 1);
    let cmd1 = CommandInteger::from(Command::LineTo, points.len() as u32 - 1);

    let (mut cx, mut cy) = points[0];

    encoded.push(cmd0);
    encoded.push(ParameterInteger::from(cx));
    encoded.push(ParameterInteger::from(cy));
    encoded.push(cmd1);

    for (x, y) in points.iter().skip(1) {
        let dx = x - cx;
        let dy = y - cy;
        encoded.push(ParameterInteger::from(dx));
        encoded.push(ParameterInteger::from(dy));
        cx = *x;
        cy = *y;
    }

    encoded
}

pub fn encode_multilinestring(linestrings: &[Vec<(i32, i32)>]) -> Vec<u32> {
    let mut encoded = Vec::new();
    let mut cx = 0;
    let mut cy = 0;
    for linestring in linestrings {
        encoded.push(CommandInteger::from(Command::MoveTo, 1));
        let first_point = linestring[0];
        let dx = first_point.0 - cx;
        let dy = first_point.1 - cy;
        encoded.push(ParameterInteger::from(dx));
        encoded.push(ParameterInteger::from(dy));
        cx = first_point.0;
        cy = first_point.1;

        encoded.push(CommandInteger::from(
            Command::LineTo,
            linestring.len() as u32 - 1,
        ));
        for (x, y) in linestring.iter().skip(1) {
            let dx = x - cx;
            let dy = y - cy;
            encoded.push(ParameterInteger::from(dx));
            encoded.push(ParameterInteger::from(dy));
            cx = *x;
            cy = *y;
        }
    }
    encoded
}

// TODO: holes etc. details from MVT spec + GeoJSON spec
pub fn encode_polygon(rings: &[Vec<(i32, i32)>]) -> Vec<u32> {
    let mut encoded = Vec::new();

    // assume first ring is exterior, others are interior
    // TODO: fix/verify assumption
    let mut exterior_rings = Vec::new(); // MVT standard: exterior CW
    let mut interior_rings = Vec::new(); // MVT standard, interior CCW
    for ring in rings {
        if ring_area(ring) > 0 {
            exterior_rings.push(ring);
        } else {
            interior_rings.push(ring);
        }
    }

    let mut cx = 0;
    let mut cy = 0;

    for ring in exterior_rings.into_iter().chain(interior_rings.into_iter()) {
        encoded.push(CommandInteger::from(Command::MoveTo, 1));
        let first_point = ring[0];
        let dx = first_point.0 - cx;
        let dy = first_point.1 - cy;
        encoded.push(ParameterInteger::from(dx));
        encoded.push(ParameterInteger::from(dy));
        cx = first_point.0;
        cy = first_point.1;

        encoded.push(CommandInteger::from(Command::LineTo, ring.len() as u32 - 2));
        for (x, y) in ring[1..ring.len() - 1].iter() {
            let dx = x - cx;
            let dy = y - cy;
            encoded.push(ParameterInteger::from(dx));
            encoded.push(ParameterInteger::from(dy));
            cx = *x;
            cy = *y;
        }
        encoded.push(CommandInteger::from(Command::ClosePath, 1));
    }

    encoded
}

pub fn encode_multipolygon(polygons: &[Vec<Vec<(i32, i32)>>]) -> Vec<u32> {
    let mut encoded = Vec::new();

    let mut cx = 0;
    let mut cy = 0;

    for polygon in polygons {
        // TODO: fix/verify assumption
        let mut exterior_rings = Vec::new(); // MVT standard: exterior CW
        let mut interior_rings = Vec::new(); // MVT standard, interior CCW

        for ring in polygon {
            if ring_area(ring) > 0 {
                exterior_rings.push(ring);
            } else {
                interior_rings.push(ring);
            }
        }

        for ring in exterior_rings.into_iter().chain(interior_rings.into_iter()) {
            encoded.push(CommandInteger::from(Command::MoveTo, 1));
            let first_point = ring[0];
            let dx = first_point.0 - cx;
            let dy = first_point.1 - cy;
            encoded.push(ParameterInteger::from(dx));
            encoded.push(ParameterInteger::from(dy));
            cx = first_point.0;
            cy = first_point.1;

            encoded.push(CommandInteger::from(Command::LineTo, ring.len() as u32 - 2));
            for (x, y) in ring[1..ring.len() - 1].iter() {
                let dx = x - cx;
                let dy = y - cy;
                encoded.push(ParameterInteger::from(dx));
                encoded.push(ParameterInteger::from(dy));
                cx = *x;
                cy = *y;
            }
            encoded.push(CommandInteger::from(Command::ClosePath, 1));
        }
    }

    encoded
}

/// ref: Surveyor's formula
fn ring_area(points: &[(i32, i32)]) -> i32 {
    let points: Vec<_> = points.iter().collect();
    let sum: i32 = points
        .windows(2)
        .map(|e| e[0].0 * e[1].1 - e[1].0 * e[0].1)
        .sum();
    return sum / 2;
}

#[test]
fn test_point_encoding() {
    assert_eq!(encode_point(25, 17), vec![9, 50, 34])
}

#[test]
fn test_multipoint_encoding() {
    let points = vec![(5, 7), (3, 2)];
    assert_eq!(encode_multipoint(&points), vec![17, 10, 14, 3, 9])
}

#[test]
fn test_linestring_encoding() {
    let linestring = vec![(2, 2), (2, 10), (10, 10)];
    assert_eq!(
        encode_linestring(&linestring),
        vec![9, 4, 4, 18, 0, 16, 16, 0]
    )
}

#[test]
fn test_multilinestring_encoding() {
    let multilinestring = vec![vec![(2, 2), (2, 10), (10, 10)], vec![(1, 1), (3, 5)]];
    assert_eq!(
        encode_multilinestring(&multilinestring),
        vec![9, 4, 4, 18, 0, 16, 16, 0, 9, 17, 17, 10, 4, 8]
    )
}

#[test]
fn test_polygon_encoding() {
    let rings = vec![vec![(3, 6), (8, 12), (20, 34), (3, 6)]];
    assert_eq!(
        encode_polygon(&rings),
        vec![9, 6, 12, 18, 10, 12, 24, 44, 15]
    );
}

#[test]
fn test_multipolygon_encoding() {
    let polygons: Vec<Vec<Vec<(i32, i32)>>> = vec![
        // Polygon 1
        vec![
            // Exterior Ring
            vec![
                (0, 0),
                (10, 0),
                (10, 10),
                (0, 10),
                (0, 0), // Path closing point
            ],
        ],
        // Polygon 2
        vec![
            // Exterior Ring
            vec![
                (11, 11),
                (20, 11),
                (20, 20),
                (11, 20),
                (11, 11), // Path closing point
            ],
            // Interior Ring (hole)
            vec![
                (13, 13),
                (13, 17),
                (17, 17),
                (17, 13),
                (13, 13), // Path closing point
            ],
        ],
    ];

    assert_eq!(
        encode_multipolygon(&polygons),
        vec![
            9, 0, 0, 26, 20, 0, 0, 20, 19, 0, 15, 9, 22, 2, 26, 18, 0, 0, 18, 17, 0, 15, 9, 4, 13,
            26, 0, 8, 8, 0, 0, 7, 15
        ]
    );
}
