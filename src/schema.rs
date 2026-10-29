//! Minimal parser for the community `dat-schema` GraphQL files (the `poe2`
//! flavour). Only the subset needed to lay out `.datc64` rows is understood.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::{Context, Result};

#[derive(Debug, Clone, PartialEq)]
pub enum Ty {
    Bool,
    I16,
    U16,
    I32,
    U32,
    F32,
    String,
    /// Opaque bytes with no known meaning.
    Raw(usize),
    /// Foreign key without a known table (16 bytes).
    Rid,
    Enum(String),
    /// Foreign key into another table (16 bytes).
    Ref(String),
    List(Box<Ty>),
}

impl Ty {
    pub fn size(&self) -> usize {
        match self {
            Ty::Bool => 1,
            Ty::I16 | Ty::U16 => 2,
            Ty::I32 | Ty::U32 | Ty::F32 | Ty::Enum(_) => 4,
            Ty::String => 8,
            Ty::Raw(n) => *n,
            Ty::Ref(_) | Ty::Rid | Ty::List(_) => 16,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Column {
    /// `None` for anonymous `_` columns.
    pub name: Option<String>,
    pub ty: Ty,
    pub offset: usize,
}

#[derive(Debug, Clone)]
pub struct TableSchema {
    pub name: String,
    pub columns: Vec<Column>,
    pub row_size: usize,
}

/// Layout fixes for the supported game build.
const OVERRIDES: &str = include_str!("../schema_overrides.txt");

#[derive(Default)]
pub struct Schema {
    /// Keyed by lower-cased table name.
    pub tables: HashMap<String, TableSchema>,
}

fn parse_ty(raw: &str, enums: &HashSet<String>) -> Ty {
    let raw = raw.trim();
    if let Some(inner) = raw.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
        return Ty::List(Box::new(parse_ty(inner, enums)));
    }
    match raw {
        "bool" => Ty::Bool,
        "i16" => Ty::I16,
        "u16" => Ty::U16,
        "i32" => Ty::I32,
        "u32" => Ty::U32,
        "f32" => Ty::F32,
        "string" => Ty::String,
        "rid" => Ty::Rid,
        "_" => Ty::Rid,
        e if enums.contains(e) => Ty::Enum(e.to_string()),
        other => Ty::Ref(other.to_string()),
    }
}

fn enum_names(src: &str, into: &mut HashSet<String>) {
    into.extend(
        src.lines()
            .filter_map(|l| l.strip_prefix("enum "))
            .filter_map(|l| l.split_whitespace().next())
            .map(str::to_string),
    );
}

impl Schema {
    /// Load from a `dat-schema/dat-schema` checkout: tables come from
    /// `poe2/_Core.gql`, but many enums they use are declared only in the
    /// shared files next to it, so enum names are gathered from all of them.
    pub fn load(root: &Path) -> Result<Self> {
        let mut extra = HashSet::new();
        for dir in [root.to_path_buf(), root.join("poe2")] {
            for entry in std::fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))? {
                let path = entry?.path();
                if path.extension().is_some_and(|e| e == "gql") {
                    enum_names(&std::fs::read_to_string(&path)?, &mut extra);
                }
            }
        }
        let core = std::fs::read_to_string(root.join("poe2").join("_Core.gql"))
            .context("reading poe2/_Core.gql")?;
        let mut schema = Self::parse(&core, extra)?;
        // POE2_OVERRIDES=<file> replaces the built-in fixes (for trying layouts without rebuilding).
        match std::env::var_os("POE2_OVERRIDES") {
            Some(f) => schema.apply_overrides(&std::fs::read_to_string(f)?)?,
            None => schema.apply_overrides(OVERRIDES)?,
        }
        Ok(schema)
    }

    /// Apply layout fixes (see `schema_overrides.txt`).
    pub fn apply_overrides(&mut self, text: &str) -> Result<()> {
        for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
            let mut it = line.split_whitespace();
            let (Some(table), Some(op)) = (it.next(), it.next()) else { continue };
            let nums = it.map(str::parse::<usize>).collect::<Result<Vec<_>, _>>()
                .with_context(|| format!("bad override: {line}"))?;
            let ts = self.tables.get_mut(&table.to_ascii_lowercase())
                .with_context(|| format!("override for unknown table {table}"))?;
            match (op, nums.as_slice()) {
                ("drop", idx) if !idx.is_empty() => {
                    let mut i = 0;
                    ts.columns.retain(|_| { i += 1; !idx.contains(&(i - 1)) });
                }
                ("insert", [at, bytes]) => {
                    let col = Column { name: None, ty: Ty::Raw(*bytes), offset: 0 };
                    ts.columns.insert((*at).min(ts.columns.len()), col);
                }
                _ => anyhow::bail!("bad override: {line}"),
            }
            let mut off = 0;
            for c in &mut ts.columns {
                c.offset = off;
                off += c.ty.size();
            }
            ts.row_size = off;
        }
        Ok(())
    }

    pub fn parse(src: &str, mut enums: HashSet<String>) -> Result<Self> {
        enum_names(src, &mut enums);

        let mut tables = HashMap::new();
        let mut cur: Option<(String, Vec<Column>, usize)> = None;
        for line in src.lines() {
            let t = line.trim();
            if let Some(rest) = t.strip_prefix("type ") {
                let name = rest
                    .split(|c: char| c.is_whitespace() || c == '{')
                    .next()
                    .context("type without name")?;
                cur = Some((name.to_string(), Vec::new(), 0));
            } else if t == "}" {
                if let Some((name, columns, row_size)) = cur.take() {
                    tables.insert(name.to_ascii_lowercase(), TableSchema { name, columns, row_size });
                }
            } else if let Some((_, cols, off)) = cur.as_mut() {
                let Some((fname, rest)) = t.split_once(':') else { continue };
                if fname.starts_with('#') || fname.contains(' ') {
                    continue;
                }
                let ty_raw = rest.trim().split(|c: char| c.is_whitespace() || c == '@').next().unwrap_or("");
                if ty_raw.is_empty() {
                    continue;
                }
                let ty = parse_ty(ty_raw, &enums);
                let name = (fname != "_").then(|| fname.to_string());
                let size = ty.size();
                cols.push(Column { name, ty, offset: *off });
                *off += size;
            }
        }
        // Fill in row sizes (offset of the end of the last column).
        for table in tables.values_mut() {
            table.row_size = table.columns.last().map_or(0, |c| c.offset + c.ty.size());
        }
        Ok(Self { tables })
    }

    pub fn table(&self, name: &str) -> Option<&TableSchema> {
        self.tables.get(&name.to_ascii_lowercase())
    }
}
