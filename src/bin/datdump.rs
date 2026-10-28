//! `datdump <dat-schema dir> <table.datc64> [rows]` or `datdump <dat-schema dir> --check <dir>`.

use std::path::Path;

use anyhow::{bail, Context, Result};
use poe2_bundles::dat::Table;
use poe2_bundles::schema::Schema;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let schema_path = args.get(1).context("usage: datdump <dat-schema dir> <table.datc64 | --check dir> [rows]")?;
    let schema = Schema::load(Path::new(schema_path))?;
    let target = args.get(2).context("missing table path")?;

    if target == "--check" {
        let dir = args.get(3).context("missing dir")?;
        let (mut ok, mut bad, mut decode_fail, mut noschema) = (0, 0, 0, 0);
        for e in std::fs::read_dir(dir)? {
            let p = e?.path();
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            if name.contains("__") || !name.ends_with(".datc64") {
                continue;
            }
            let stem = name.trim_end_matches(".datc64");
            let Some(ts) = schema.table(stem) else { noschema += 1; continue };
            let data = std::fs::read(&p)?;
            let t = Table::parse(&data)?;
            if t.rows > 0 && t.row_size != ts.row_size {
                bad += 1;
                println!("mismatch: {stem} file={} schema={}", t.row_size, ts.row_size);
                continue;
            }
            let all_ok = (0..t.rows).all(|i| t.decode_row(ts, i).is_ok());
            if all_ok { ok += 1 } else { decode_fail += 1; println!("decode failure: {stem}") }
        }
        println!("fully decoded: {ok}, size mismatch: {bad}, decode failures: {decode_fail}, no schema: {noschema}");
        return Ok(());
    }

    let limit: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(3);
    let data = std::fs::read(target)?;
    let stem = Path::new(target).file_stem().unwrap().to_string_lossy().to_string();
    let t = Table::parse(&data)?;
    let ts = schema.table(&stem).with_context(|| format!("no schema for {stem}"))?;
    if t.rows > 0 && t.row_size != ts.row_size {
        bail!("{stem}: row is {} bytes in the file but the schema describes {}", t.row_size, ts.row_size);
    }
    println!("{stem}: {} rows", t.rows);
    for i in 0..t.rows.min(limit) {
        println!("-- row {i}");
        for (n, v) in t.decode_row(ts, i)? {
            println!("  {n}: {v:?}");
        }
    }
    Ok(())
}
