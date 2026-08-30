//! Benchmarks hillshading

#![allow(clippy::cast_possible_truncation)]

use std::hint::black_box;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, ImageEncoder as _};
use martin_core::tiles::hillshade::{BakeParams, Canvas, LightAngles, bake_with_light};
use martin_core::tiles::neighbourhood::{NEIGHBOURHOOD_LEN, Neighbourhood};
use martin_tile_utils::{Format, TileData};

/// Side length of a Mapzen normal tile.
const TILE_SIZE: u32 = 256;

/// Synthesizes a plausible Mapzen-style *normal* tile, PNG-encoded.
fn synthetic_normal_tile(width: u32, height: u32, seed: u32) -> TileData {
    fn hash(x: u32, y: u32, seed: u32) -> u32 {
        let mut h = x.wrapping_mul(0x9E37_79B1)
            ^ y.wrapping_mul(0x85EB_CA77)
            ^ seed.wrapping_mul(0xC2B2_AE3D);
        h ^= h >> 15;
        h = h.wrapping_mul(0x2C1B_3C6D);
        h ^= h >> 12;
        h = h.wrapping_mul(0x297A_2D39);
        h ^= h >> 15;
        h
    }

    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let nx = hash(x, y, seed) as u8;
            let ny = hash(x, y, seed.wrapping_add(1)) as u8;
            let ramp = (x + y) * 255 / (width + height).max(1);
            let elevation = (ramp as u8).wrapping_add((hash(x, y, seed.wrapping_add(7)) as u8) / 8);
            pixels.extend_from_slice(&[nx, ny, 128, elevation]);
        }
    }
    let mut buf = Vec::new();
    PngEncoder::new(&mut buf)
        .write_image(&pixels, width, height, ExtendedColorType::Rgba8)
        .expect("encode synthetic normal tile");
    buf
}

/// A full 3x3 neighbourhood of distinct synthetic tiles.
fn full_neighbourhood(seed: u32) -> Neighbourhood {
    let tiles: [Option<TileData>; NEIGHBOURHOOD_LEN] =
        std::array::from_fn(|i| Some(synthetic_normal_tile(TILE_SIZE, TILE_SIZE, seed + i as u32)));
    Neighbourhood::from_row_major(tiles)
}

fn bench_decode(c: &mut Criterion) {
    let bytes = synthetic_normal_tile(TILE_SIZE, TILE_SIZE, 1);
    c.bench_function("decode_one_normal_tile", |b| {
        b.iter(|| {
            let img = image::load_from_memory(black_box(&bytes))
                .expect("decode")
                .into_rgba8();
            black_box(img);
        });
    });
}

fn bench_assemble(c: &mut Criterion) {
    let neighbourhood = full_neighbourhood(100);
    c.bench_function("assemble_neighbourhood_9_tiles", |b| {
        b.iter(|| {
            let canvas = Canvas::from_neighbourhood(black_box(&neighbourhood)).expect("assemble");
            black_box(canvas);
        });
    });
}

fn bench_bake(c: &mut Criterion) {
    let neighbourhood = full_neighbourhood(200);
    let canvas = Canvas::from_neighbourhood(&neighbourhood).expect("assemble");
    let params = BakeParams::default();
    let light = LightAngles::default().to_vector();

    let mut group = c.benchmark_group("bake_with_light");
    for core_side in [256_u32, 512] {
        group.bench_function(format!("core_{core_side}"), |b| {
            b.iter(|| {
                let baked = bake_with_light(black_box(&canvas), core_side, &params, light);
                black_box(baked);
            });
        });
    }
    group.finish();
}

/// Cost of `band_hard`'s quantisation step in isolation.
fn bench_banding_overhead(c: &mut Criterion) {
    let neighbourhood = full_neighbourhood(250);
    let canvas = Canvas::from_neighbourhood(&neighbourhood).expect("assemble");
    let light = LightAngles::default().to_vector();

    let mut group = c.benchmark_group("banding_overhead_core_512");
    for (label, toon_bands) in [("off", 0.0), ("on", 3.0)] {
        let params = BakeParams {
            toon_bands,
            ..BakeParams::default()
        };
        group.bench_function(label, |b| {
            b.iter(|| black_box(bake_with_light(black_box(&canvas), 512, &params, light)));
        });
    }
    group.finish();
}

fn bench_encode(c: &mut Criterion) {
    let neighbourhood = full_neighbourhood(300);
    let canvas = Canvas::from_neighbourhood(&neighbourhood).expect("assemble");
    let light = LightAngles::default().to_vector();
    let baked = bake_with_light(&canvas, 512, &BakeParams::default(), light);

    let mut group = c.benchmark_group("encode");
    for format in [Format::Png, Format::Webp] {
        group.bench_function(format!("{format:?}"), |b| {
            b.iter(|| {
                let bytes = baked.encode(black_box(format)).expect("encode");
                black_box(bytes);
            });
        });
    }
    group.finish();
}

fn bench_end_to_end(c: &mut Criterion) {
    let neighbourhood = full_neighbourhood(400);
    let light = LightAngles::default().to_vector();

    let mut group = c.benchmark_group("end_to_end_uncached_bake");
    for core_side in [256_u32, 512] {
        for format in [Format::Png, Format::Webp] {
            group.bench_function(format!("core_{core_side}_{format:?}"), |b| {
                b.iter_batched(
                    || neighbourhood.clone(),
                    |neighbourhood| {
                        let canvas = Canvas::from_neighbourhood(&neighbourhood).expect("assemble");
                        let baked =
                            bake_with_light(&canvas, core_side, &BakeParams::default(), light);
                        let bytes = baked.encode(format).expect("encode");
                        black_box(bytes);
                    },
                    BatchSize::SmallInput,
                );
            });
        }
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_decode,
    bench_assemble,
    bench_bake,
    bench_banding_overhead,
    bench_encode,
    bench_end_to_end
);
criterion_main!(benches);
