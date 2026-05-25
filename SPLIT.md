# Splitting `static-rendering-squashed` into reviewable chunks

The branch's 38 commits are all `wip` -- unusable for a commit-based split. Split by
**content** instead. Five logical units; only ① and ② are independent of each other,
③④⑤ must stack. Recommended landing order: **① → ② → ③ → ④ → ⑤**.

Total: ~3210 insertions / 188 deletions across 110 files (63 are binary PNG fixtures).

---

## ① Camera hardening + test coverage
*Independent. Smallest. Can land first against `main`.*

Bug fixes to the **existing** GET static endpoint plus its test matrix.

**Files (partial -- `styles_static.rs` is shared with ⑤):**
- `martin/src/srv/styles_static.rs` -- take only:
  - `SizeRequest::validate`: the non-finite/`<= 0` scale guard (NaN/Inf/negative currently
    saturate to `0u8` and slip past the `MAX_SCALE` check, then feed NaN/Inf into
    `log2(pixel_ratio)`). Keep the reworded `#[expect]` reason.
  - `CameraRequest::validate` (new `impl`): inverted-bbox rejection.
  - In `handle_static_request`: the `path.camera.validate()` call (keep the GET signature
    `(path, styles)` -- **do not** add the `overlays` param here; that's ⑤).
  - Tests: `zero_scale`/`negative_scale`/`nan_scale`/`pos_inf_scale`/`neg_inf_scale` cases
    and `inverted_bbox_returns_400`. **Exclude** all `post_*` tests.
- `martin/tests/styles_rendering_test.rs` -- the camera half only:
  - `static_camera_expected/` → `static_camera/` rename (`CAMERA_EXPECTED_DIR`→`CAMERA_DIR`)
  - the `test_each_path!` camera matrix. **Exclude** every `overlay`/`post_*` symbol.
- `tests/fixtures/static_camera/*.png` -- 7 files (`git mv` from `static_camera_expected/`
  where they already exist; net-new ones added).
- `Cargo.toml` -- add `test_each_file = "0.3"` to `[workspace.dependencies]`.
- `martin/Cargo.toml` -- add `test_each_file.workspace = true` under `[dev-dependencies]`.

**Verify:** `cargo test -p martin --features rendering --test styles_rendering_test`
(Linux + MLN; see `[[project_mln_precompile]]` -- unset `MLN_PRECOMPILE` to build MLN from source).

---

## ② Dependency bump
*Prerequisite for ③④⑤. No behavior change; must still compile + pass CI on its own.*

- `Cargo.toml` (`[workspace.dependencies]`):
  - `maplibre_native` `"0.4.4"` → `{ version = "0.6.0", features = ["geojson"] }`
  - `serde-saphyr` `0.0.25` → `0.0.26`
  - add `csscolorparser = "0.8"`, `geojson = "1"` (used by ④⑤; declaring early is harmless)
- `justfile`:
  - `check`: prefix `MLN_PRECOMPILE=1`
  - extract `stable_features := '...'`; rewrite `build-deb` / `build-release-musl` to use it
  - drop the `cargo update --precise 1.44.3 insta` pin
- `Cargo.lock` -- regenerate (`cargo update -p maplibre_native -p serde-saphyr` + the new deps).

**Risk:** confirm the **unmodified** `render_pool.rs` still compiles against MLN 0.6. If 0.6
changed the renderer API, the minimal adapting hunk must ride along in ② (don't pull the
whole ③ restructure in). Check before splitting.

**Verify:** `just check` (cargo-hack each-feature) + `just lint`.

---

## ③ Multi-threaded render pool
*Depends on ②. Breaking `StyleSources` API change.*

- `martin-core/src/resources/styles/render_pool.rs` -- **partial**. Take the threading half:
  `RenderPool` struct, `Inner` + its `Drop`, `RenderPool::new(workers)`, `render`,
  `default_worker_count`, `worker_loop`, `WorkerMsg`, `WORKER_QUEUE_DEPTH`, the
  multi-threaded test. **Leave for ⑤:** `RenderParams::with_overlays`, `RenderOverlay`,
  its `Drop`, and the overlay-application body inside `render_one` (keep a `render_one` that
  renders the base map with no overlay step).
- `martin-core/src/resources/styles/mod.rs` -- `pool: Option<RenderPool>` field;
  `is_rendering_enabled` via `pool.is_some()`; `enable_rendering(workers)` /
  `disable_rendering()` replacing `set_rendering_enabled(bool)`; updated `render_static`;
  doc tweaks; test using `enable_rendering(None)`.
- `martin-core/Cargo.toml` -- add `flume` (optional) to the `rendering` feature.
- `martin/src/config/file/resources/styles.rs` -- `RendererConfig.workers: Option<NonZeroUsize>`;
  rewire the `OptBoolObj` match to `enable_rendering(o.workers)` / `disable_rendering()`;
  the two `workers` parse tests.
- `martin/src/config/file/error.rs` -- `ConfigFileError::RenderPoolSpawnFailed` + diagnostic code.
- **Regenerated:** `schemas/config.json` (+`workers`), `docs/content/files/generated_config.md`
  -- produce via `just gen-schemas`, don't hand-edit.

**Verify:** `cargo test -p martin-core --features rendering`,
`cargo test -p martin --features rendering config::`, `just gen-schemas` clean.

---

## ④ Overlay parsing (pure)
*Depends only on ②'s `csscolorparser`/`geojson`. No renderer -- fully unit-testable.* ~910 lines.

- `martin-core/src/lib.rs` -- `#[cfg(feature = "overlay")] pub mod overlay;`
- `martin-core/src/overlay/mod.rs` -- types (`OverlaySpec`, `OverlayFeature`, ids) + `mod parse;`
  and the `pub use parse::{...}` only. **Hold back** `mod apply;` / `pub use apply::{...}` until ⑤.
- `martin-core/src/overlay/parse.rs` -- GeoJSON + simplestyle → `OverlaySpec`.
- `martin-core/tests/overlay_parse_test.rs` -- 414 lines, pure parser tests.
- `martin-core/Cargo.toml` -- new `overlay = ["dep:csscolorparser", "dep:geojson", "dep:serde_json"]`
  feature; the optional dep table entries. **Do not** yet add `overlay` to the `rendering`
  feature (that coupling belongs in ⑤).

**Verify:** `cargo test -p martin-core --features overlay --test overlay_parse_test`.

---

## ⑤ Overlay rendering + POST endpoint
*Headline feature. Depends on ②③④. Largest.*

- `martin-core/src/overlay/apply.rs` -- `OverlaySpec` → MapLibre `Style` mutations
  (`ApplyError`, `apply_to_style`); needs MLN 0.6 `geojson`.
- `martin-core/src/overlay/mod.rs` -- add `mod apply;` + `pub use apply::{...}` (the held-back lines).
- `martin-core/Cargo.toml` -- add `overlay` + `dep:flume` to the `rendering` feature.
- `martin-core/src/resources/styles/render_pool.rs` -- the overlay half held back in ③:
  `RenderParams::with_overlays(Arc<OverlaySpec>)`, `RenderOverlay` + its `Drop`, and the
  overlay-application step inside `render_one`.
- `martin-core/src/resources/styles/error.rs` -- `StyleError::OverlayApply(#[from] ApplyError)`.
- `martin/src/srv/styles_static.rs` -- the POST half: `OverlayBody` extractor,
  `post_rendered_static_style`, utoipa schema structs (`StaticStyleOverlay` etc.),
  `render_base`→`render_with_overlays` rename + `overlays` param threaded through
  `handle_static_request`, the `OverlayApply` 400 arm, the `trace!`→structured-`debug!` swap,
  `POST` added to the jpeg redirect; all `post_*` tests.
- `martin/src/srv/mod.rs`, `martin/src/srv/server.rs`, `martin/src/schemas.rs` -- register the
  POST route + its `__path_*` export + OpenAPI path.
- `martin/Cargo.toml` -- `geojson` (optional) added to the `rendering` feature + dep table.
- `martin/tests/styles_rendering_test.rs` -- overlay half: `post_png_body`/`post_no_body`,
  `run_overlay_scenario`, the four `test_each_path!` overlay matrices (1x / 2x / pitch / bearing),
  `empty_body_renders_base_map`, `empty_overlay_renders_base_map`.
- `tests/fixtures/static_overlays/` -- 56 PNG + 14 input JSON.
- `docs/content/sources-styles.md` (+228) and `docs/content/images/static-overlay/*.png`;
  `zensical.toml` (+1).
- **Regenerated:** `schemas/openapi.json`, `martin/martin-ui/src/lib/types.gen.ts`,
  `martin/martin-ui/package.json` -- via `just gen-schemas`; don't hand-edit.

**Verify:** `cargo test -p martin-core --features rendering`,
`cargo test -p martin --features rendering --test styles_rendering_test`,
`just gen-schemas && just test-schemas`, then `just lint`.

---

## Cross-cutting notes

- **`render_pool.rs` is the only file split mid-file** (③ threading vs ⑤ overlay). Everything
  else splits cleanly per file or per hunk. Do ③'s version first with a no-op base-map
  `render_one`, then ⑤ layers the overlay step back in.
- **`styles_static.rs` and `styles_rendering_test.rs`** each appear in three chunks
  (①/⑤ for the source, ①/⑤ for the test). The seam is GET-validation/camera (①) vs
  POST/overlay (⑤) -- non-overlapping hunks.
- **`CHANGELOG.md`:** out of scope -- the user writes those entries. (`[[feedback_no_changelog]]`)
- **Generated artifacts** (`schemas/*.json`, `generated_config.md`, `types.gen.ts`,
  `package.json`) should be produced by `just gen-schemas` in whichever chunk introduces the
  underlying change (③ for config, ⑤ for openapi/UI), never hand-edited.
- **Platform:** rendering chunks (③⑤) are `#[cfg(all(feature = "rendering", target_os = "linux"))]`
  and need MapLibre Native -- build from source per `[[project_mln_precompile]]`.
