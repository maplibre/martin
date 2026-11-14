//! Times Martin's render pools on the operations Martin serves; one case per process.
//!
//! `render-bench <case> <style.json> [workers]`, printing one JSON line.

#![expect(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::many_single_char_names,
    clippy::panic,
    clippy::print_stdout,
    clippy::too_many_lines,
    reason = "benchmark helper"
)]

use std::f64::consts::PI;
use std::num::{NonZeroU8, NonZeroUsize};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::future::join_all;
use geojson::{Geometry, GeometryValue};
use martin_core::overlay::{Color, OverlayFeature, OverlayProperties, OverlaySpec};
use martin_core::styles::render_pool::RenderPools;
use martin_core::styles::{RenderParams, TileSize};

const BERLIN: (f64, f64) = (52.52, 13.405);

fn tile_of(lat: f64, lon: f64, z: u8) -> (u32, u32) {
    let n = f64::from(1_u32 << z);
    let x = ((lon + 180.0) / 360.0 * n) as u32;
    let lat = lat.to_radians();
    let y = ((1.0 - (lat.tan() + 1.0 / lat.cos()).ln() / PI) / 2.0 * n) as u32;
    (x, y)
}

/// A `side` x `side` block of tiles at `z` centered on Berlin, row by row.
fn block(z: u8, side: u32) -> Vec<(u8, u32, u32)> {
    let (cx, cy) = tile_of(BERLIN.0, BERLIN.1, z);
    let half = side / 2;
    let mut tiles = Vec::new();
    for y in 0..side {
        for x in 0..side {
            tiles.push((z, (cx + x).saturating_sub(half), (cy + y).saturating_sub(half)));
        }
    }
    tiles
}

fn low_zooms() -> Vec<(u8, u32, u32)> {
    let mut tiles = Vec::new();
    for z in 0..=3_u8 {
        for y in 0..(1_u32 << z) {
            for x in 0..(1_u32 << z) {
                tiles.push((z, x, y));
            }
        }
    }
    tiles
}

fn cpu_seconds() -> f64 {
    let stat = std::fs::read_to_string("/proc/self/stat").expect("stat");
    let fields: Vec<&str> = stat.rsplit_once(')').expect("comm").1.split_whitespace().collect();
    let ticks: f64 = fields[11].parse::<f64>().expect("utime") + fields[12].parse::<f64>().expect("stime");
    ticks / 100.0
}

fn max_rss_mib() -> f64 {
    let status = std::fs::read_to_string("/proc/self/status").expect("status");
    let line = status.lines().find(|l| l.starts_with("VmHWM:")).expect("VmHWM");
    line.split_whitespace()
        .nth(1)
        .expect("kB")
        .parse::<f64>()
        .expect("number")
        / 1024.0
}

fn rss_mib() -> f64 {
    let status = std::fs::read_to_string("/proc/self/status").expect("status");
    let line = status.lines().find(|l| l.starts_with("VmRSS:")).expect("VmRSS");
    line.split_whitespace()
        .nth(1)
        .expect("kB")
        .parse::<f64>()
        .expect("number")
        / 1024.0
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

struct Timings {
    samples: Vec<f64>,
}

impl Timings {
    fn json(&mut self) -> String {
        self.samples.sort_by(f64::total_cmp);
        let s = &self.samples;
        let mean = s.iter().sum::<f64>() / s.len() as f64;
        format!(
            r#""n":{},"p50_ms":{:.2},"p90_ms":{:.2},"min_ms":{:.2},"max_ms":{:.2},"mean_ms":{:.2}"#,
            s.len(),
            percentile(s, 0.5),
            percentile(s, 0.9),
            s[0],
            s[s.len() - 1],
            mean
        )
    }
}

fn overlay(features: usize) -> Arc<OverlaySpec> {
    let color = Color {
        r: 0.9,
        g: 0.2,
        b: 0.1,
        a: 0.8,
    };
    let mut out = Vec::new();
    for i in 0..features {
        let dx = (i % 10) as f64 * 0.004 - 0.02;
        let dy = (i / 10) as f64 * 0.003 - 0.015;
        let (lat, lon) = (BERLIN.0 + dy, BERLIN.1 + dx);
        let (geometry, properties) = match i % 3 {
            0 => (
                GeometryValue::new_point([lon, lat]),
                OverlayProperties {
                    circle_color: Some(color),
                    circle_radius: Some(6.0),
                    circle_stroke_width: Some(1.0),
                    ..OverlayProperties::default()
                },
            ),
            1 => (
                GeometryValue::new_line_string([[lon, lat], [lon + 0.003, lat + 0.002], [lon + 0.004, lat - 0.001]]),
                OverlayProperties {
                    line_color: Some(color),
                    line_width: Some(3.0),
                    ..OverlayProperties::default()
                },
            ),
            _ => (
                GeometryValue::new_polygon([[
                    [lon, lat],
                    [lon + 0.003, lat],
                    [lon + 0.003, lat + 0.002],
                    [lon, lat + 0.002],
                    [lon, lat],
                ]]),
                OverlayProperties {
                    fill_color: Some(color),
                    fill_opacity: Some(0.5),
                    line_color: Some(color),
                    ..OverlayProperties::default()
                },
            ),
        };
        out.push(OverlayFeature {
            geometry: Some(Geometry::new(geometry)),
            properties: Some(properties),
        });
    }
    Arc::new(OverlaySpec { features: out })
}

fn statics() -> Vec<(f64, f64, f64, f64, f64)> {
    (0..12)
        .map(|i| {
            let f = f64::from(i);
            (
                BERLIN.0 + (f * 0.37).sin() * 0.02,
                BERLIN.1 + (f * 0.53).cos() * 0.03,
                13.0 + (f % 3.0),
                f * 30.0,
                (f % 4.0) * 15.0,
            )
        })
        .collect()
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let case = args[1].as_str();
    let style = PathBuf::from(&args[2]);
    let workers: usize = args.get(3).map_or(1, |w| w.parse().expect("workers"));
    let save = std::env::var("BENCH_SAVE").ok().map(PathBuf::from);
    let one = NonZeroU8::MIN;
    let two = NonZeroU8::new(2).expect("2");
    let workers_nz = NonZeroUsize::new(workers).expect("workers > 0");

    let cpu0 = cpu_seconds();
    let started = Instant::now();
    let renderers = if case == "style-switch" {
        NonZeroUsize::MIN
    } else {
        NonZeroUsize::new(8).expect("8")
    };
    let pools = RenderPools::new(Some(workers_nz), TileSize::Px512, renderers, None).expect("pools");
    let startup_ms = ms(started.elapsed());
    let rss_after_start = rss_mib();
    let mut extra = String::new();

    let mut t = Timings { samples: Vec::new() };
    let save_png = |name: &str, img: &image::RgbaImage| {
        if let Some(dir) = &save {
            std::fs::create_dir_all(dir).expect("dir");
            img.save(dir.join(format!("{name}.png"))).expect("save");
        }
    };

    let tile_case = |size: TileSize, ratio: NonZeroU8| (size, ratio);
    let (kind, rest) = case.split_once(':').unwrap_or((case, ""));
    match kind {
        "startup" => t.samples.push(startup_ms),
        // First tile of a fresh pool: style load, every resource it needs, render, readback.
        "cold-tile" => {
            let (z, x, y) = block(14, 1)[0];
            let s = Instant::now();
            let img = pools.render_tile(style.clone(), z, x, y).await.expect("render");
            t.samples.push(ms(s.elapsed()));
            save_png("cold", &img);
        }
        // The same tile again and again: placement, draw and readback only.
        "warm-tile" => {
            let (size, ratio) = match rest {
                "256@1" => tile_case(TileSize::Px256, one),
                "512@2" => tile_case(TileSize::Px512, two),
                _ => tile_case(TileSize::Px512, one),
            };
            let (z, x, y) = block(14, 1)[0];
            pools
                .render_tile_of_size(style.clone(), z, x, y, size, ratio)
                .await
                .expect("warmup");
            for _ in 0..40 {
                let s = Instant::now();
                pools
                    .render_tile_of_size(style.clone(), z, x, y, size, ratio)
                    .await
                    .expect("render");
                t.samples.push(ms(s.elapsed()));
            }
        }
        // Distinct tiles one after another, as a tile server's traffic is: each needs its data parsed and laid out.
        "sweep" => {
            let (zs, size, ratio) = match rest {
                "z14-256@1" => (14, TileSize::Px256, one),
                "z14-512@2" => (14, TileSize::Px512, two),
                "z10-512@1" => (10, TileSize::Px512, one),
                "z6-512@1" => (6, TileSize::Px512, one),
                _ => (14, TileSize::Px512, one),
            };
            let tiles = block(zs, 8);
            let (z, x, y) = (zs, tiles[0].1 + 20, tiles[0].2 + 20);
            pools
                .render_tile_of_size(style.clone(), z, x, y, size, ratio)
                .await
                .expect("style warmup");
            for (z, x, y) in tiles {
                let s = Instant::now();
                let img = pools
                    .render_tile_of_size(style.clone(), z, x, y, size, ratio)
                    .await
                    .expect("render");
                t.samples.push(ms(s.elapsed()));
                save_png(&format!("sweep-{rest}-{z}-{x}-{y}"), &img);
            }
        }
        "lowzoom" => {
            pools.render_tile(style.clone(), 5, 17, 10).await.expect("style warmup");
            for (z, x, y) in low_zooms() {
                let s = Instant::now();
                let img = pools.render_tile(style.clone(), z, x, y).await.expect("render");
                t.samples.push(ms(s.elapsed()));
                save_png(&format!("low-{z}-{x}-{y}"), &img);
            }
        }
        // Free-camera images with bearing and pitch, at retina size.
        "static" => {
            let (w, h) = (1024, 768);
            pools
                .render_static(RenderParams::new(style.clone(), BERLIN.0, BERLIN.1, 10.0).with_size(w, h, two))
                .await
                .expect("warmup");
            for (i, (lat, lon, zoom, bearing, pitch)) in statics().into_iter().enumerate() {
                let params = RenderParams::new(style.clone(), lat, lon, zoom)
                    .with_size(w, h, two)
                    .with_orientation(bearing, pitch);
                let s = Instant::now();
                let img = pools.render_static(params).await.expect("render");
                t.samples.push(ms(s.elapsed()));
                save_png(&format!("static-{i}"), &img);
            }
        }
        // A static image with an overlay added before and removed after, over a view already loaded.
        "overlay" => {
            let n: usize = rest.parse().unwrap_or(60);
            let spec = overlay(n);
            let base = || RenderParams::new(style.clone(), BERLIN.0, BERLIN.1, 14.0).with_size(800, 600, one);
            pools.render_static(base()).await.expect("warmup");
            let mut plain = Vec::new();
            for _ in 0..15 {
                let s = Instant::now();
                pools.render_static(base()).await.expect("render");
                plain.push(ms(s.elapsed()));
            }
            for i in 0..15 {
                let s = Instant::now();
                let img = pools
                    .render_static(base().with_overlays(Arc::clone(&spec)))
                    .await
                    .expect("render");
                t.samples.push(ms(s.elapsed()));
                if i == 0 {
                    save_png("overlay", &img);
                }
            }
            plain.sort_by(f64::total_cmp);
            extra = format!(r#","plain_p50_ms":{:.2}"#, percentile(&plain, 0.5));
        }
        // One renderer per worker, two styles in turn: every request reloads its style.
        "style-switch" => {
            let other = PathBuf::from(rest);
            let (z, x, y) = block(14, 1)[0];
            pools.render_tile(style.clone(), z, x, y).await.expect("warmup a");
            pools.render_tile(other.clone(), z, x, y).await.expect("warmup b");
            for i in 0..12 {
                let s = if i % 2 == 0 { style.clone() } else { other.clone() };
                let st = Instant::now();
                pools.render_tile(s, z, x, y).await.expect("render");
                t.samples.push(ms(st.elapsed()));
            }
        }
        // Every tile of a block at once, as concurrent clients ask: requests per second over all workers.
        "throughput" => {
            let (zs, side) = match rest {
                "z15" => (15, 16),
                _ => (14, 16),
            };
            let warm = block(zs, 2);
            join_all(
                warm.iter()
                    .map(|&(z, x, y)| pools.render_tile(style.clone(), z, x + 40, y + 40)),
            )
            .await;
            let tiles = block(zs, side);
            let s = Instant::now();
            let results = join_all(tiles.iter().map(|&(z, x, y)| {
                let style = style.clone();
                let pools = &pools;
                async move {
                    let st = Instant::now();
                    pools.render_tile(style, z, x, y).await.map(|_| ms(st.elapsed()))
                }
            }))
            .await;
            let wall = s.elapsed().as_secs_f64();
            for r in results {
                t.samples.push(r.expect("render"));
            }
            extra = format!(r#","rps":{:.2},"wall_s":{wall:.3}"#, t.samples.len() as f64 / wall);
        }
        other => panic!("unknown case {other}"),
    }
    let cpu = cpu_seconds() - cpu0;
    let wall = started.elapsed().as_secs_f64();
    println!(
        r#"{{"case":"{case}","style":"{}","workers":{workers},{},"startup_ms":{startup_ms:.1},"cpu_s":{cpu:.2},"wall_s_total":{wall:.2},"rss_start_mib":{rss_after_start:.0},"maxrss_mib":{:.0}{extra}}}"#,
        style.file_stem().and_then(|s| s.to_str()).unwrap_or("?"),
        t.json(),
        max_rss_mib()
    );
    drop(pools);
}
