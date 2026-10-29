//! Monster statistics from the `MonsterVarieties` table.

use std::path::Path;

use anyhow::{Context, Result};

use crate::dat::{Table, Value};
use crate::index::Index;
use crate::schema::Schema;

#[derive(Debug, Clone, Default)]
pub struct MonsterStats {
    pub id: String,
    pub name: String,
    /// Percent of the base life of the monster's level.
    pub life_multiplier: i64,
    pub damage_multiplier: i64,
    pub movement_speed: i64,
    /// Attacks per 1000 seconds, as stored.
    pub attack_speed: i64,
    pub min_attack_distance: i64,
    pub max_attack_distance: i64,
    pub aggro_range: i64,
}

/// Find the variety whose Id or AO file list matches `ao_path` (an `.ao` path or a bare Id).
pub fn find(idx: &Index, dir: &Path, schema_dir: &Path, ao_path: &str) -> Result<MonsterStats> {
    let schema = Schema::load(schema_dir)?;
    let bytes = idx.read_file(dir, "data/balance/monstervarieties.datc64")?;
    let table = Table::parse(&bytes)?;
    let ts = schema.table("monstervarieties").context("no schema for monstervarieties")?;
    let want = ao_path.to_ascii_lowercase();
    let stem = want.rsplit('/').next().unwrap_or(&want).trim_end_matches(".ao").to_string();
    let int = |row: &[(String, Value)], n: &str| match row.iter().find(|(k, _)| k == n) {
        Some((_, Value::Int(v))) => *v,
        _ => 0,
    };
    let text = |row: &[(String, Value)], n: &str| match row.iter().find(|(k, _)| k == n) {
        Some((_, Value::Str(s))) => s.clone(),
        _ => String::new(),
    };
    for i in 0..table.rows {
        let row = table.decode_row(ts, i)?;
        let id = text(&row, "Id");
        let ids = id.to_ascii_lowercase();
        let in_ao = row.iter().any(|(k, v)| {
            k == "AOFiles" && matches!(v, Value::List(l) if l.iter().any(|x| matches!(x, Value::Str(s) if s.to_ascii_lowercase() == want)))
        });
        if in_ao || ids.ends_with(&format!("/{stem}")) || ids == stem {
            return Ok(MonsterStats {
                id,
                name: text(&row, "Name"),
                life_multiplier: int(&row, "LifeMultiplier"),
                damage_multiplier: int(&row, "DamageMultiplier"),
                movement_speed: int(&row, "MovementSpeed"),
                attack_speed: int(&row, "AttackSpeed"),
                min_attack_distance: int(&row, "MinimumAttackDistance"),
                max_attack_distance: int(&row, "MaximumAttackDistance"),
                aggro_range: int(&row, "MinAgroRange"),
            });
        }
    }
    anyhow::bail!("no monster variety for {ao_path}")
}

/// Base life and damage of a monster of `level` from `DefaultMonsterStats` (row 0 is level 1).
pub fn level_stats(idx: &Index, dir: &Path, schema_dir: &Path, level: usize) -> Result<(i64, f64)> {
    let schema = Schema::load(schema_dir)?;
    let bytes = idx.read_file(dir, "data/balance/defaultmonsterstats.datc64")?;
    let table = Table::parse(&bytes)?;
    let ts = schema.table("defaultmonsterstats").context("no schema for defaultmonsterstats")?;
    let row = table.decode_row(ts, level.saturating_sub(1).min(table.rows.saturating_sub(1)))?;
    let life = row.iter().find_map(|(k, v)| match (k.as_str(), v) { ("MonsterLife", Value::Int(i)) => Some(*i), _ => None }).unwrap_or(0);
    let dmg = row.iter().find_map(|(k, v)| match (k.as_str(), v) { ("Damage", Value::Float(f)) => Some(*f as f64), _ => None }).unwrap_or(0.0);
    Ok((life, dmg))
}

/// Starting values of a player class from the `Characters` table.
#[derive(Debug, Clone, Copy)]
pub struct PlayerStats {
    pub life: i64,
    pub min_damage: i64,
    pub max_damage: i64,
    /// Milliseconds per unarmed attack, as stored in `WeaponSpeed`.
    pub attack_ms: i64,
}

/// Find the class whose Id is a prefix of the character's `.ao` file name (e.g. `strfourb` -> `StrFour`).
pub fn player_stats(idx: &Index, dir: &Path, schema_dir: &Path, ao_path: &str) -> Result<PlayerStats> {
    let schema = Schema::load(schema_dir)?;
    let bytes = idx.read_file(dir, "data/balance/characters.datc64")?;
    let table = Table::parse(&bytes)?;
    let ts = schema.table("characters").context("no schema for characters")?;
    let want = ao_path.to_ascii_lowercase();
    let stem = want.rsplit('/').next().unwrap_or(&want).trim_end_matches(".ao").to_string();
    let mut best: Option<(usize, PlayerStats)> = None;
    for i in 0..table.rows {
        let row = table.decode_row(ts, i)?;
        let get = |n: &str| row.iter().find_map(|(k, v)| match v { Value::Int(x) if k == n => Some(*x), _ => None }).unwrap_or(0);
        let id = row.iter().find_map(|(k, v)| match v { Value::Str(s) if k == "Id" => Some(s.to_ascii_lowercase()), _ => None }).unwrap_or_default();
        let name = id.rsplit('/').next().unwrap_or("").to_string();
        if !name.is_empty() && stem.starts_with(&name) && best.as_ref().is_none_or(|(l, _)| name.len() > *l) {
            best = Some((name.len(), PlayerStats { life: get("BaseMaxLife"), min_damage: get("MinDamage"), max_damage: get("MaxDamage"), attack_ms: get("WeaponSpeed") }));
        }
    }
    best.map(|(_, s)| s).with_context(|| format!("no class for {ao_path}"))
}
