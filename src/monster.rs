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
