//! The review of `martin test generate` in a terminal.
#![cfg(all(feature = "test-pg", feature = "test-generate"))]

use std::fs;

use indoc::indoc;
use martin_e2e_tests::{MartinTestReview, seed_postgres, temp_dir};

#[tokio::test]
async fn a_changed_case_shows_its_output_against_the_spec() {
    seed_postgres(indoc! {"
        DROP SCHEMA IF EXISTS martin_review_changed CASCADE;
        CREATE SCHEMA martin_review_changed;
        CREATE TABLE martin_review_changed.places (id int8 PRIMARY KEY, kind text, geom geometry(Point, 4326));
        INSERT INTO martin_review_changed.places VALUES (1, 'major', 'SRID=4326;POINT(16.37 48.20)');
    "})
    .await;
    let dir = temp_dir();
    fs::write(
        dir.path().join("config.yaml"),
        indoc! {"
            postgres:
              connection_string: ${DATABASE_URL}
              auto_publish: false
              tables:
                places:
                  schema: martin_review_changed
                  table: places
                  srid: 4326
                  geometry_column: geom
                  geometry_type: POINT
                  id_column: id
                  properties:
                    id: int8
                    kind: text
                  layers:
                    places:
                      minzoom: 5
                      attributes:
                        kind: kind
        "},
    )
    .unwrap();
    fs::write(
        dir.path().join("spec.yaml"),
        indoc! {"
            max_zoom: 14
            cases:
              - source: places
                paths: ['places: every feature']
                input: { geometry: POINT(16.37 48.2), id: 1, kind: major }
                output:
                  - { layer: places, id: 1, geometry: point, zooms: 4-14, attributes: { kind: major } }
        "},
    )
    .unwrap();
    let review = MartinTestReview::start(dir.path(), &["--config", "config.yaml", "--output", "spec.yaml"]);
    insta::assert_snapshot!(review.screen_with("Reviewing [1/1]"), @r#"
    " Reviewing [1/1]  CHANGED  places 1  undecided                                                      "
    " the row or what bulk mode renders of it differs from the spec, accept takes the new case           "
    "                                                                                                    "
    " paths   places: every feature                                                                      "
    "                                                                                                    "
    " -spec +now                                                                                         "
    "   1    1 │ - layer: places                                                                         "
    "   2    2 │   id: 1                                                                                 "
    "   3    3 │   geometry: point                                                                       "
    "   4      │-  zooms: 4-14                                                                           "
    "        4 │+  zooms: 5-14                                                                           "
    "   5    5 │   attributes:                                                                           "
    "   6    6 │     kind: major                                                                         "
    "                                                                                                    "
    "                                                                                                    "
    " a accept  r reject  A accept all undecided  <-/-> prev/next  ↑/↓ scroll  q quit without writing    "
    "#);
}

#[tokio::test]
async fn a_case_with_more_paths_than_the_screen_scrolls_to_its_diff() {
    seed_postgres(indoc! {"
        DROP SCHEMA IF EXISTS martin_review_scroll CASCADE;
        CREATE SCHEMA martin_review_scroll;
        CREATE TABLE martin_review_scroll.places (id int8 PRIMARY KEY, geom geometry(Point, 4326));
        INSERT INTO martin_review_scroll.places VALUES (2, 'SRID=4326;POINT(16.37 48.20)');
    "})
    .await;
    let dir = temp_dir();
    fs::write(
        dir.path().join("config.yaml"),
        indoc! {"
            postgres:
              connection_string: ${DATABASE_URL}
              auto_publish: false
              tables:
                places:
                  schema: martin_review_scroll
                  table: places
                  srid: 4326
                  geometry_column: geom
                  geometry_type: POINT
                  id_column: id
                  properties:
                    id: int8
                  layers:
                    places:
                      attributes:
                        a01: \"id < 11 ? 'yes' : 'no'\"
                        a02: \"id < 12 ? 'yes' : 'no'\"
                        a03: \"id < 13 ? 'yes' : 'no'\"
                        a04: \"id < 14 ? 'yes' : 'no'\"
                        a05: \"id < 15 ? 'yes' : 'no'\"
                        a06: \"id < 16 ? 'yes' : 'no'\"
                        a07: \"id < 17 ? 'yes' : 'no'\"
                        a08: \"id < 18 ? 'yes' : 'no'\"
                        a09: \"id < 19 ? 'yes' : 'no'\"
                        a10: \"id < 20 ? 'yes' : 'no'\"
                        a11: \"id < 21 ? 'yes' : 'no'\"
                        a12: \"id < 22 ? 'yes' : 'no'\"
                        a13: \"id < 23 ? 'yes' : 'no'\"
                        a14: \"id < 24 ? 'yes' : 'no'\"
                        a15: \"id < 25 ? 'yes' : 'no'\"
        "},
    )
    .unwrap();
    let mut review = MartinTestReview::start(dir.path(), &["--config", "config.yaml", "--output", "spec.yaml"]);
    review.wait_for("Reviewing [1/1]");
    review.press("jjjjjjjjjjjj");
    insta::assert_snapshot!(review.screen_with(r#"a01: "yes""#), @r#"
    " Reviewing [1/1]  NEW  places 2  undecided                                                          "
    " not in the spec, accept adds it                                                                    "
    "         places: attributes.a12 'yes', `id < 22` is true                                            "
    "         places: attributes.a13 'yes', `id < 23` is true                                            "
    "         places: attributes.a14 'yes', `id < 24` is true                                            "
    "         places: attributes.a15 'yes', `id < 25` is true                                            "
    "                                                                                                    "
    " -spec +now                                                                                         "
    "   1      │-(not in the spec)                                                                       "
    "        1 │+- layer: places                                                                         "
    "        2 │+  id: 2                                                                                 "
    "        3 │+  geometry: point                                                                       "
    "        4 │+  zooms: 0-14                                                                           "
    "        5 │+  attributes:                                                                           "
    "        6 │+    a01: "yes"                                                                          "
    " a accept  r reject  A accept all undecided  <-/-> prev/next  ↑/↓ scroll  q quit without writing    "
    "#);
}

#[tokio::test]
async fn a_changed_value_wider_than_the_screen_wraps() {
    seed_postgres(indoc! {"
        DROP SCHEMA IF EXISTS martin_review_wrap CASCADE;
        CREATE SCHEMA martin_review_wrap;
        CREATE TABLE martin_review_wrap.places (id int8 PRIMARY KEY, name text, geom geometry(Point, 4326));
        INSERT INTO martin_review_wrap.places VALUES
          (1, 'the road that runs from the old harbour past the station and the market to the castle on the hill', 'SRID=4326;POINT(16.37 48.20)');
    "})
    .await;
    let dir = temp_dir();
    fs::write(
        dir.path().join("config.yaml"),
        indoc! {"
            postgres:
              connection_string: ${DATABASE_URL}
              auto_publish: false
              tables:
                places:
                  schema: martin_review_wrap
                  table: places
                  srid: 4326
                  geometry_column: geom
                  geometry_type: POINT
                  id_column: id
                  properties:
                    id: int8
                    name: text
                  layers:
                    places:
                      attributes:
                        note: \"name + ', then south'\"
        "},
    )
    .unwrap();
    fs::write(
        dir.path().join("spec.yaml"),
        indoc! {"
            max_zoom: 14
            cases:
              - source: places
                paths: ['places: every feature']
                input:
                  geometry: POINT(16.37 48.2)
                  id: 1
                  name: the road that runs from the old harbour past the station and the market to the castle on the hill
                output:
                  - layer: places
                    id: 1
                    geometry: point
                    zooms: 0-14
                    attributes:
                      note: the road that runs from the old harbour past the station and the market to the castle on the hill, then north
        "},
    )
    .unwrap();
    let review = MartinTestReview::start(dir.path(), &["--config", "config.yaml", "--output", "spec.yaml"]);
    insta::assert_snapshot!(review.screen_with("Reviewing [1/1]"), @r#"
    " Reviewing [1/1]  CHANGED  places 1  undecided                                                      "
    " the row or what bulk mode renders of it differs from the spec, accept takes the new case           "
    "                                                                                                    "
    " paths   places: every feature                                                                      "
    "                                                                                                    "
    " -spec +now                                                                                         "
    "   1    1 │ - layer: places                                                                         "
    "   2    2 │   id: 1                                                                                 "
    "   3    3 │   geometry: point                                                                       "
    "   4    4 │   zooms: 0-14                                                                           "
    "   5    5 │   attributes:                                                                           "
    "   6      │-    note: the road that runs from the old harbour past the station and the market to the"
    "castle on the hill, then north                                                                      "
    "        6 │+    note: the road that runs from the old harbour past the station and the market to the"
    "castle on the hill, then south                                                                      "
    " a accept  r reject  A accept all undecided  <-/-> prev/next  ↑/↓ scroll  q quit without writing    "
    "#);
}

#[tokio::test]
async fn a_new_case_shows_its_output_against_no_case() {
    seed_postgres(indoc! {"
        DROP SCHEMA IF EXISTS martin_review_new CASCADE;
        CREATE SCHEMA martin_review_new;
        CREATE TABLE martin_review_new.places (id int8 PRIMARY KEY, geom geometry(Point, 4326));
        INSERT INTO martin_review_new.places VALUES (2, 'SRID=4326;POINT(16.37 48.20)');
    "})
    .await;
    let dir = temp_dir();
    fs::write(
        dir.path().join("config.yaml"),
        indoc! {"
            postgres:
              connection_string: ${DATABASE_URL}
              auto_publish: false
              tables:
                places:
                  schema: martin_review_new
                  table: places
                  srid: 4326
                  geometry_column: geom
                  geometry_type: POINT
                  id_column: id
                  properties:
                    id: int8
                  layers:
                    places: {}
        "},
    )
    .unwrap();
    let review = MartinTestReview::start(dir.path(), &["--config", "config.yaml", "--output", "spec.yaml"]);
    insta::assert_snapshot!(review.screen_with("Reviewing [1/1]"), @r#"
    " Reviewing [1/1]  NEW  places 2  undecided                                                          "
    " not in the spec, accept adds it                                                                    "
    "                                                                                                    "
    " paths   places: every feature                                                                      "
    "                                                                                                    "
    " -spec +now                                                                                         "
    "   1      │-(not in the spec)                                                                       "
    "        1 │+- layer: places                                                                         "
    "        2 │+  id: 2                                                                                 "
    "        3 │+  geometry: point                                                                       "
    "        4 │+  zooms: 0-14                                                                           "
    "        5 │+  attributes:                                                                           "
    "        6 │+    id: 2                                                                               "
    "                                                                                                    "
    "                                                                                                    "
    " a accept  r reject  A accept all undecided  <-/-> prev/next  ↑/↓ scroll  q quit without writing    "
    "#);
}

#[tokio::test]
async fn the_last_screen_lists_the_decisions_and_the_paths_without_a_row() {
    seed_postgres(indoc! {"
        DROP SCHEMA IF EXISTS martin_review_last CASCADE;
        CREATE SCHEMA martin_review_last;
        CREATE TABLE martin_review_last.places (id int8 PRIMARY KEY, kind text, name text, geom geometry(Point, 4326));
        INSERT INTO martin_review_last.places VALUES
          (1, 'major', 'B', 'SRID=4326;POINT(16.37 48.20)'),
          (2, 'minor', 'C', 'SRID=4326;POINT(16.38 48.21)');
    "})
    .await;
    let dir = temp_dir();
    fs::write(
        dir.path().join("config.yaml"),
        indoc! {"
            postgres:
              connection_string: ${DATABASE_URL}
              auto_publish: false
              tables:
                places:
                  schema: martin_review_last
                  table: places
                  srid: 4326
                  geometry_column: geom
                  geometry_type: POINT
                  id_column: id
                  properties:
                    id: int8
                    kind: text
                    name: text
                  layers:
                    places:
                      minzoom: \"kind == 'major' ? 5 : 8\"
                      attributes:
                        short: \"name + '!' == 'A!' ? 'A' : name\"
        "},
    )
    .unwrap();
    fs::write(
        dir.path().join("spec.yaml"),
        indoc! {"
            max_zoom: 14
            cases:
              - source: places
                input: { geometry: POINT(16.37 48.2), id: 1, kind: major, name: B }
                output:
                  - { layer: places, id: 1, geometry: point, zooms: 4-14, attributes: { short: B } }
              - source: places
                input: { geometry: POINT(16.39 48.22), id: 9, kind: major, name: B }
                output: []
        "},
    )
    .unwrap();
    let mut review = MartinTestReview::start(dir.path(), &["--config", "config.yaml", "--output", "spec.yaml"]);
    review.wait_for("Reviewing [1/3]");
    review.press("arn");
    insta::assert_snapshot!(review.screen_with("Reviewed"), @r#"
    " Reviewed  spec.yaml                                                                                "
    "                                                                                                    "
    " accepted     1  places 1                                                                           "
    " rejected     1  places 2  kept as in the spec                                                      "
    " undecided    1  places 9  kept as in the spec                                                      "
    " same         0                                                                                     "
    "                                                                                                    "
    " 3 paths take no row, so no case covers them                                                        "
    "                                                                                                    "
    " NO ROW  places  places: attributes.short 'A', `(name + '!') == 'A!'` is true                       "
    "     none of 2 rows swept                                                                           "
    "     no condition selects the rows, it uses `+`, which has no SQL equivalent                        "
    "                                                                                                    "
    " NO ROW  places  places: attributes.short, `name` is null in `(name + '!') == 'A!'`                 "
    "     no row where "name" IS NULL                                                                    "
    " w write spec.yaml  <- back  ↑/↓ scroll  q quit without writing                                     "
    "#);
}

#[tokio::test]
async fn rejected_and_undecided_cases_keep_the_spec() {
    seed_postgres(indoc! {"
        DROP SCHEMA IF EXISTS martin_review_keep CASCADE;
        CREATE SCHEMA martin_review_keep;
        CREATE TABLE martin_review_keep.places (id int8 PRIMARY KEY, kind text, geom geometry(Point, 4326));
        INSERT INTO martin_review_keep.places VALUES
          (1, 'major', 'SRID=4326;POINT(16.37 48.20)'),
          (2, 'minor', 'SRID=4326;POINT(16.38 48.21)');
    "})
    .await;
    let dir = temp_dir();
    fs::write(
        dir.path().join("config.yaml"),
        indoc! {"
            postgres:
              connection_string: ${DATABASE_URL}
              auto_publish: false
              tables:
                places:
                  schema: martin_review_keep
                  table: places
                  srid: 4326
                  geometry_column: geom
                  geometry_type: POINT
                  id_column: id
                  properties:
                    id: int8
                    kind: text
                  layers:
                    places:
                      minzoom: \"kind == 'major' ? 5 : 8\"
        "},
    )
    .unwrap();
    fs::write(
        dir.path().join("spec.yaml"),
        indoc! {"
            max_zoom: 14
            cases:
              - source: places
                input: { geometry: POINT(16.37 48.2), id: 1, kind: major }
                output:
                  - { layer: places, id: 1, geometry: point, zooms: 4-14 }
              - source: places
                input: { geometry: POINT(16.39 48.22), id: 9, kind: major }
                output: []
        "},
    )
    .unwrap();
    let mut review = MartinTestReview::start(dir.path(), &["--config", "config.yaml", "--output", "spec.yaml"]);
    review.wait_for("Reviewing [1/3]");
    review.press("arnw");
    let exit = review.exit_code();
    let spec = fs::read_to_string(dir.path().join("spec.yaml")).unwrap();
    insta::assert_snapshot!(format!("exit: {exit}\n\n{spec}"), @r#"
    exit: 0

    max_zoom: 14
    cases:
    - source: places
      paths:
      - "places: minzoom 5, `kind == 'major'` is true"
      input:
        geometry: POINT(16.37 48.2)
        id: 1
        kind: major
      output:
      - layer: places
        id: 1
        geometry: point
        zooms: 5-14
        attributes:
          id: 1
          kind: major
    - source: places
      input:
        geometry: POINT(16.39 48.22)
        id: 9
        kind: major
      output: []
    "#);
}

#[tokio::test]
async fn a_gone_case_shows_that_accepting_removes_it() {
    seed_postgres(indoc! {"
        DROP SCHEMA IF EXISTS martin_review_gone CASCADE;
        CREATE SCHEMA martin_review_gone;
        CREATE TABLE martin_review_gone.places (id int8 PRIMARY KEY, kind text, geom geometry(Point, 4326));
    "})
    .await;
    let dir = temp_dir();
    fs::write(
        dir.path().join("config.yaml"),
        indoc! {"
            postgres:
              connection_string: ${DATABASE_URL}
              auto_publish: false
              tables:
                places:
                  schema: martin_review_gone
                  table: places
                  srid: 4326
                  geometry_column: geom
                  geometry_type: POINT
                  id_column: id
                  properties:
                    id: int8
                    kind: text
                  layers:
                    places:
                      minzoom: 12
        "},
    )
    .unwrap();
    fs::write(
        dir.path().join("spec.yaml"),
        indoc! {"
            max_zoom: 14
            cases:
              - source: places
                paths: ['places: every feature']
                input: { geometry: POINT(16.39 48.22), id: 9, kind: major }
                output:
                  - { layer: places, id: 9, geometry: point, zooms: 12-14 }
        "},
    )
    .unwrap();
    let review = MartinTestReview::start(dir.path(), &["--config", "config.yaml", "--output", "spec.yaml"]);
    insta::assert_snapshot!(review.screen_with("Reviewing [1/1]"), @r#"
    " Reviewing [1/1]  GONE  places 9  undecided                                                         "
    " in the spec, but no path takes this row any more, accept removes it                                "
    "                                                                                                    "
    " paths   places: every feature                                                                      "
    "                                                                                                    "
    " -spec +now                                                                                         "
    "   1      │-- layer: places                                                                         "
    "   2      │-  id: 9                                                                                 "
    "   3      │-  geometry: point                                                                       "
    "   4      │-  zooms: 12-14                                                                          "
    "        1 │+(no path takes the row any more)                                                        "
    "                                                                                                    "
    "                                                                                                    "
    "                                                                                                    "
    "                                                                                                    "
    " a accept  r reject  A accept all undecided  <-/-> prev/next  ↑/↓ scroll  q quit without writing    "
    "#);
}

#[tokio::test]
async fn accepting_all_undecided_and_writing_writes_every_generated_case_and_drops_the_gone() {
    seed_postgres(indoc! {"
        DROP SCHEMA IF EXISTS martin_review_accept_all CASCADE;
        CREATE SCHEMA martin_review_accept_all;
        CREATE TABLE martin_review_accept_all.places (id int8 PRIMARY KEY, kind text, geom geometry(Point, 4326));
        INSERT INTO martin_review_accept_all.places VALUES
          (1, 'major', 'SRID=4326;POINT(16.37 48.20)'),
          (2, 'minor', 'SRID=4326;POINT(16.38 48.21)');
    "})
    .await;
    let dir = temp_dir();
    fs::write(
        dir.path().join("config.yaml"),
        indoc! {"
            postgres:
              connection_string: ${DATABASE_URL}
              auto_publish: false
              tables:
                places:
                  schema: martin_review_accept_all
                  table: places
                  srid: 4326
                  geometry_column: geom
                  geometry_type: POINT
                  id_column: id
                  properties:
                    id: int8
                    kind: text
                  layers:
                    places:
                      minzoom: \"kind == 'major' ? 5 : 8\"
        "},
    )
    .unwrap();
    fs::write(
        dir.path().join("spec.yaml"),
        indoc! {"
            max_zoom: 14
            cases:
              - source: places
                input: { geometry: POINT(16.37 48.2), id: 1, kind: major }
                output:
                  - { layer: places, id: 1, geometry: point, zooms: 4-14 }
              - source: places
                input: { geometry: POINT(16.39 48.22), id: 9, kind: major }
                output: []
        "},
    )
    .unwrap();
    let mut review = MartinTestReview::start(dir.path(), &["--config", "config.yaml", "--output", "spec.yaml"]);
    review.wait_for("Reviewing [1/3]");
    review.press("Aw");
    let exit = review.exit_code();
    let spec = fs::read_to_string(dir.path().join("spec.yaml")).unwrap();
    insta::assert_snapshot!(format!("exit: {exit}\n\n{spec}"), @r#"
    exit: 0

    max_zoom: 14
    cases:
    - source: places
      paths:
      - "places: minzoom 5, `kind == 'major'` is true"
      input:
        geometry: POINT(16.37 48.2)
        id: 1
        kind: major
      output:
      - layer: places
        id: 1
        geometry: point
        zooms: 5-14
        attributes:
          id: 1
          kind: major
    - source: places
      paths:
      - "places: minzoom 8, `kind == 'major'` is false"
      input:
        geometry: POINT(16.38 48.21)
        id: 2
        kind: minor
      output:
      - layer: places
        id: 2
        geometry: point
        zooms: 8-14
        attributes:
          id: 2
          kind: minor
    "#);
}

#[tokio::test]
async fn quitting_after_a_decision_writes_nothing() {
    seed_postgres(indoc! {"
        DROP SCHEMA IF EXISTS martin_review_quit CASCADE;
        CREATE SCHEMA martin_review_quit;
        CREATE TABLE martin_review_quit.places (id int8 PRIMARY KEY, kind text, geom geometry(Point, 4326));
        INSERT INTO martin_review_quit.places VALUES
          (1, 'major', 'SRID=4326;POINT(16.37 48.20)'),
          (2, 'minor', 'SRID=4326;POINT(16.38 48.21)');
    "})
    .await;
    let dir = temp_dir();
    fs::write(
        dir.path().join("config.yaml"),
        indoc! {"
            postgres:
              connection_string: ${DATABASE_URL}
              auto_publish: false
              tables:
                places:
                  schema: martin_review_quit
                  table: places
                  srid: 4326
                  geometry_column: geom
                  geometry_type: POINT
                  id_column: id
                  properties:
                    id: int8
                    kind: text
                  layers:
                    places:
                      minzoom: \"kind == 'major' ? 5 : 8\"
        "},
    )
    .unwrap();
    fs::write(
        dir.path().join("spec.yaml"),
        indoc! {"
            max_zoom: 14
            cases:
              - source: places
                input: { geometry: POINT(16.37 48.2), id: 1, kind: major }
                output:
                  - { layer: places, id: 1, geometry: point, zooms: 4-14 }
        "},
    )
    .unwrap();
    let mut review = MartinTestReview::start(dir.path(), &["--config", "config.yaml", "--output", "spec.yaml"]);
    review.wait_for("Reviewing [1/2]");
    review.press("aq");
    let exit = review.exit_code();
    let spec = fs::read_to_string(dir.path().join("spec.yaml")).unwrap();
    insta::assert_snapshot!(format!("exit: {exit}\n\n{spec}"), @"
    exit: 1

    max_zoom: 14
    cases:
      - source: places
        input: { geometry: POINT(16.37 48.2), id: 1, kind: major }
        output:
          - { layer: places, id: 1, geometry: point, zooms: 4-14 }
    ");
}

#[tokio::test]
async fn writing_before_the_last_screen_does_nothing() {
    seed_postgres(indoc! {"
        DROP SCHEMA IF EXISTS martin_review_write_early CASCADE;
        CREATE SCHEMA martin_review_write_early;
        CREATE TABLE martin_review_write_early.places (id int8 PRIMARY KEY, kind text, geom geometry(Point, 4326));
        INSERT INTO martin_review_write_early.places VALUES
          (1, 'major', 'SRID=4326;POINT(16.37 48.20)'),
          (2, 'minor', 'SRID=4326;POINT(16.38 48.21)');
    "})
    .await;
    let dir = temp_dir();
    fs::write(
        dir.path().join("config.yaml"),
        indoc! {"
            postgres:
              connection_string: ${DATABASE_URL}
              auto_publish: false
              tables:
                places:
                  schema: martin_review_write_early
                  table: places
                  srid: 4326
                  geometry_column: geom
                  geometry_type: POINT
                  id_column: id
                  properties:
                    id: int8
                    kind: text
                  layers:
                    places:
                      minzoom: \"kind == 'major' ? 5 : 8\"
        "},
    )
    .unwrap();
    fs::write(
        dir.path().join("spec.yaml"),
        indoc! {"
            max_zoom: 14
            cases:
              - source: places
                input: { geometry: POINT(16.37 48.2), id: 1, kind: major }
                output:
                  - { layer: places, id: 1, geometry: point, zooms: 4-14 }
        "},
    )
    .unwrap();
    let mut review = MartinTestReview::start(dir.path(), &["--config", "config.yaml", "--output", "spec.yaml"]);
    review.wait_for("Reviewing [1/2]");
    review.press("wq");
    let exit = review.exit_code();
    let spec = fs::read_to_string(dir.path().join("spec.yaml")).unwrap();
    insta::assert_snapshot!(format!("exit: {exit}\n\n{spec}"), @"
    exit: 1

    max_zoom: 14
    cases:
      - source: places
        input: { geometry: POINT(16.37 48.2), id: 1, kind: major }
        output:
          - { layer: places, id: 1, geometry: point, zooms: 4-14 }
    ");
}

#[tokio::test]
async fn a_case_kept_at_another_max_zoom_than_the_cases_blocks_the_write() {
    seed_postgres(indoc! {"
        DROP SCHEMA IF EXISTS martin_review_zoom_blocked CASCADE;
        CREATE SCHEMA martin_review_zoom_blocked;
        CREATE TABLE martin_review_zoom_blocked.places (id int8 PRIMARY KEY, geom geometry(Point, 4326));
        INSERT INTO martin_review_zoom_blocked.places VALUES (1, 'SRID=4326;POINT(16.37 48.20)');
    "})
    .await;
    let dir = temp_dir();
    fs::write(
        dir.path().join("config.yaml"),
        indoc! {"
            postgres:
              connection_string: ${DATABASE_URL}
              auto_publish: false
              tables:
                places:
                  schema: martin_review_zoom_blocked
                  table: places
                  srid: 4326
                  geometry_column: geom
                  geometry_type: POINT
                  id_column: id
                  properties:
                    id: int8
                  layers:
                    places:
                      minzoom: 4
        "},
    )
    .unwrap();
    fs::write(
        dir.path().join("spec.yaml"),
        indoc! {"
            max_zoom: 14
            cases:
              - source: places
                paths: ['places: every feature']
                input: { geometry: POINT(16.37 48.2), id: 1 }
                output:
                  - { layer: places, id: 1, geometry: point, zooms: 4-14 }
        "},
    )
    .unwrap();
    let mut review = MartinTestReview::start(
        dir.path(),
        &["--config", "config.yaml", "--output", "spec.yaml", "--max-zoom", "12"],
    );
    review.wait_for("Reviewing [1/1]");
    review.press("rw");
    insta::assert_snapshot!(review.screen_with("Reviewed"), @r#"
    " Reviewed  spec.yaml                                                                                "
    "                                                                                                    "
    " accepted     0                                                                                     "
    " rejected     1  places 1  kept as in the spec                                                      "
    " undecided    0                                                                                     "
    " same         0                                                                                     "
    "                                                                                                    "
    " The spec has max zoom 14, these cases 12, so accept every changed and gone case to write           "
    "                                                                                                    "
    " Every path through the layers takes a row.                                                         "
    "                                                                                                    "
    "                                                                                                    "
    "                                                                                                    "
    "                                                                                                    "
    "                                                                                                    "
    " <- back  ↑/↓ scroll  q quit without writing                                                        "
    "#);
}

#[tokio::test]
async fn accepting_every_case_at_another_max_zoom_writes_the_new_max_zoom() {
    seed_postgres(indoc! {"
        DROP SCHEMA IF EXISTS martin_review_zoom_accepted CASCADE;
        CREATE SCHEMA martin_review_zoom_accepted;
        CREATE TABLE martin_review_zoom_accepted.places (id int8 PRIMARY KEY, geom geometry(Point, 4326));
        INSERT INTO martin_review_zoom_accepted.places VALUES (1, 'SRID=4326;POINT(16.37 48.20)');
    "})
    .await;
    let dir = temp_dir();
    fs::write(
        dir.path().join("config.yaml"),
        indoc! {"
            postgres:
              connection_string: ${DATABASE_URL}
              auto_publish: false
              tables:
                places:
                  schema: martin_review_zoom_accepted
                  table: places
                  srid: 4326
                  geometry_column: geom
                  geometry_type: POINT
                  id_column: id
                  properties:
                    id: int8
                  layers:
                    places:
                      minzoom: 4
        "},
    )
    .unwrap();
    fs::write(
        dir.path().join("spec.yaml"),
        indoc! {"
            max_zoom: 14
            cases:
              - source: places
                paths: ['places: every feature']
                input: { geometry: POINT(16.37 48.2), id: 1 }
                output:
                  - { layer: places, id: 1, geometry: point, zooms: 4-14 }
        "},
    )
    .unwrap();
    let mut review = MartinTestReview::start(
        dir.path(),
        &["--config", "config.yaml", "--output", "spec.yaml", "--max-zoom", "12"],
    );
    review.wait_for("Reviewing [1/1]");
    review.press("aw");
    let exit = review.exit_code();
    let spec = fs::read_to_string(dir.path().join("spec.yaml")).unwrap();
    insta::assert_snapshot!(format!("exit: {exit}\n\n{spec}"), @r#"
    exit: 0

    max_zoom: 12
    cases:
    - source: places
      paths:
      - "places: every feature"
      input:
        geometry: POINT(16.37 48.2)
        id: 1
      output:
      - layer: places
        id: 1
        geometry: point
        zooms: 4-12
        attributes:
          id: 1
    "#);
}
