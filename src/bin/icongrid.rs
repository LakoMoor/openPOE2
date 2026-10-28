//! `icongrid <Bundles2> <dat-schema dir> <table.datc64> <column> <count> <out.png>`
//! Builds a contact sheet from a table column holding `.dds` paths.

use std::io::Cursor;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use image::{imageops, RgbaImage};
use image_dds::ddsfile::Dds;
use poe2_bundles::dat::{Table, Value};
use poe2_bundles::index::Index;
use poe2_bundles::schema::Schema;

const CELL: u32 = 72;
const COLS: u32 = 16;

fn main() -> Result<()> {
    let a: Vec<String> = std::env::args().collect();
    anyhow::ensure!(a.len() == 7, "usage: icongrid <Bundles2> <schema dir> <table> <column> <count> <out.png>");
    let dir = PathBuf::from(&a[1]);
    let schema = Schema::load(Path::new(&a[2]))?;
    let data = std::fs::read(&a[3])?;
    let stem = Path::new(&a[3]).file_stem().unwrap().to_string_lossy().to_string();
    let ts = schema.table(&stem).with_context(|| format!("no schema for {stem}"))?;
    let table = Table::parse(&data)?;
    let want: usize = a[5].parse()?;
    let idx = Index::parse(&std::fs::read(dir.join("_.index.bin"))?)?;

    let mut tiles: Vec<RgbaImage> = Vec::new();
    for row in 0..table.rows {
        if tiles.len() >= want {
            break;
        }
        let cells = table.decode_row(ts, row)?;
        let Some((_, Value::Str(p))) = cells.iter().find(|(n, _)| n == &a[4]) else { continue };
        if p.is_empty() {
            continue;
        }
        let Ok(bytes) = idx.read_file(&dir, p) else { continue };
        let Ok(dds) = Dds::read(&mut Cursor::new(bytes)) else { continue };
        let Ok(img) = image_dds::image_from_dds(&dds, 0) else { continue };
        tiles.push(imageops::resize(&img, CELL - 8, CELL - 8, imageops::FilterType::Lanczos3));
    }
    let rows = (tiles.len() as u32).div_ceil(COLS);
    let mut sheet = RgbaImage::from_pixel(COLS * CELL, rows * CELL, image::Rgba([11, 8, 7, 255]));
    for (i, t) in tiles.iter().enumerate() {
        let (x, y) = ((i as u32 % COLS) * CELL + 4, (i as u32 / COLS) * CELL + 4);
        imageops::overlay(&mut sheet, t, x as i64, y as i64);
    }
    sheet.save(&a[6])?;
    println!("{} icons -> {}", tiles.len(), a[6]);
    Ok(())
}
