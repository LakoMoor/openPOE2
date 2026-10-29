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
#[derive(Debug, Clone)]
pub struct PlayerStats {
    pub life: i64,
    pub min_damage: i64,
    pub max_damage: i64,
    /// Milliseconds per unarmed attack, as stored in `WeaponSpeed`.
    pub attack_ms: i64,
    /// Cast time in ms of the class's start skill (the dodge roll), via GrantedEffectsPerLevel -> GrantedEffects.
    pub roll_ms: i64,
    /// Name of the starting weapon, when its `WeaponTypes` row was found (its damage and speed replace the unarmed ones).
    pub weapon: Option<String>,
    /// `.ao` of the weapon as held in the hand (`ItemVisualIdentity.AOFile2`), lower case.
    pub weapon_model: Option<String>,
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
    let mut roll_ref: Option<Value> = None;
    let mut weapon_ref: Option<Value> = None;
    for i in 0..table.rows {
        let row = table.decode_row(ts, i)?;
        let get = |n: &str| row.iter().find_map(|(k, v)| match v { Value::Int(x) if k == n => Some(*x), _ => None }).unwrap_or(0);
        let id = row.iter().find_map(|(k, v)| match v { Value::Str(s) if k == "Id" => Some(s.to_ascii_lowercase()), _ => None }).unwrap_or_default();
        let name = id.rsplit('/').next().unwrap_or("").to_string();
        if !name.is_empty() && stem.starts_with(&name) && best.as_ref().is_none_or(|(l, _)| name.len() > *l) {
            best = Some((name.len(), PlayerStats { life: get("BaseMaxLife"), min_damage: get("MinDamage"), max_damage: get("MaxDamage"), attack_ms: get("WeaponSpeed"), roll_ms: 0, weapon: None, weapon_model: None }));
            weapon_ref = row.iter().find_map(|(k, v)| match v { Value::List(l) if k == "StartWeapons" => l.first().cloned(), _ => None });
            roll_ref = row.iter().find_map(|(k, v)| match v { Value::List(l) if k == "StartingSkill" => l.first().cloned(), _ => None });
        }
    }
    let mut stats = best.map(|(_, s)| s).with_context(|| format!("no class for {ao_path}"))?;
    // The start skill is a GrantedEffectsPerLevel row; its GrantedEffect carries the cast time.
    if let Some(Value::Ref(Some(level_row))) = roll_ref {
        let per_bytes = idx.read_file(dir, "data/balance/grantedeffectsperlevel.datc64")?;
        let per = Table::parse(&per_bytes)?;
        let eff_bytes = idx.read_file(dir, "data/balance/grantedeffects.datc64")?;
        let eff = Table::parse(&eff_bytes)?;
        if let (Some(pts), Some(ets)) = (schema.table("grantedeffectsperlevel"), schema.table("grantedeffects")) {
            let prow = per.decode_row(pts, level_row as usize)?;
            if let Some(Value::Ref(Some(e))) = prow.iter().find(|(k, _)| k == "GrantedEffect").map(|(_, v)| v.clone()) {
                let erow = eff.decode_row(ets, e as usize)?;
                stats.roll_ms = erow.iter().find_map(|(k, v)| match v { Value::Int(x) if k == "CastTime" => Some(*x), _ => None }).unwrap_or(0);
            }
        }
    }
    // The starting weapon: BaseItemTypes row -> WeaponTypes row (damage, speed).
    if let Some(Value::Ref(Some(item))) = weapon_ref {
        let wt_bytes = idx.read_file(dir, "data/balance/weapontypes.datc64")?;
        let wt = Table::parse(&wt_bytes)?;
        if let Some(wts) = schema.table("weapontypes") {
            for i in 0..wt.rows {
                let row = wt.decode_row(wts, i)?;
                let get = |n: &str| row.iter().find_map(|(k, v)| match v { Value::Int(x) if k == n => Some(*x), _ => None });
                let is_item = row.iter().any(|(k, v)| k == "BaseItemType" && matches!(v, Value::Ref(Some(r)) if *r == item));
                if is_item {
                    stats.min_damage = get("DamageMin").unwrap_or(stats.min_damage);
                    stats.max_damage = get("DamageMax").unwrap_or(stats.max_damage);
                    stats.attack_ms = get("Speed").unwrap_or(stats.attack_ms);
                    stats.weapon_model = held_model(idx, dir, &schema, item as usize);
                    stats.weapon = item_name(idx, dir, &schema, item as usize).or_else(|| Some(format!("base item {item}")));
                    break;
                }
            }
        }
    }
    // Life grows by `life_per_level` for every level (16 + 12 = 28 at level 1).
    let level = std::env::var("POE2_LEVEL").ok().and_then(|v| v.parse::<i64>().ok()).unwrap_or(1).max(1);
    stats.life += template_value(idx, dir, "life_per_level").unwrap_or(0) * level;
    Ok(stats)
}

/// Display name of a `BaseItemTypes` row.
pub fn item_name(idx: &Index, dir: &Path, schema: &Schema, row: usize) -> Option<String> {
    let bytes = idx.read_file(dir, "data/balance/baseitemtypes.datc64").ok()?;
    let table = Table::parse(&bytes).ok()?;
    let ts = schema.table("baseitemtypes")?;
    let r = table.decode_row(ts, row).ok()?;
    r.iter().find_map(|(k, v)| match v { Value::Str(s) if k == "Name" => Some(s.clone()), _ => None })
}

/// `key = value` integer from the player template `metadata/characters/character.ot` (UTF-16 or UTF-8 text).
pub fn template_value(idx: &Index, dir: &Path, key: &str) -> Option<i64> {
    let b = idx.read_file(dir, "metadata/characters/character.ot").ok()?;
    let text = if b.starts_with(&[0xff, 0xfe]) {
        String::from_utf16(&b[2..].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect::<Vec<_>>()).ok()?
    } else {
        String::from_utf8_lossy(&b).into_owned()
    };
    text.lines().find_map(|l| {
        let (k, v) = l.split_once('=')?;
        (k.trim() == key).then(|| v.trim().parse().ok()).flatten()
    })
}

/// A weapon base type with its `WeaponTypes` numbers.
#[derive(Debug, Clone)]
pub struct Weapon {
    pub name: String,
    pub min_damage: i64,
    pub max_damage: i64,
    /// Milliseconds per attack.
    pub attack_ms: i64,
}

/// Weapon base types that can drop at `max_drop_level` or below, ordered by drop level then name.
pub fn weapons(idx: &Index, dir: &Path, schema_dir: &Path, max_drop_level: i64) -> Result<Vec<Weapon>> {
    let schema = Schema::load(schema_dir)?;
    let wt_bytes = idx.read_file(dir, "data/balance/weapontypes.datc64")?;
    let wt = Table::parse(&wt_bytes)?;
    let it_bytes = idx.read_file(dir, "data/balance/baseitemtypes.datc64")?;
    let it = Table::parse(&it_bytes)?;
    let wts = schema.table("weapontypes").context("no schema for weapontypes")?;
    let its = schema.table("baseitemtypes").context("no schema for baseitemtypes")?;
    let mut out: Vec<(i64, Weapon)> = Vec::new();
    for i in 0..wt.rows {
        let row = wt.decode_row(wts, i)?;
        let get = |n: &str| row.iter().find_map(|(k, v)| match v { Value::Int(x) if k == n => Some(*x), _ => None }).unwrap_or(0);
        let Some(Value::Ref(Some(item))) = row.iter().find(|(k, _)| k == "BaseItemType").map(|(_, v)| v.clone()) else { continue };
        let item = it.decode_row(its, item as usize)?;
        let level = item.iter().find_map(|(k, v)| match v { Value::Int(x) if k == "DropLevel" => Some(*x), _ => None }).unwrap_or(0);
        let name = item.iter().find_map(|(k, v)| match v { Value::Str(s) if k == "Name" => Some(s.clone()), _ => None }).unwrap_or_default();
        if name.is_empty() || level > max_drop_level || get("DamageMax") == 0 || get("Speed") == 0 {
            continue;
        }
        out.push((level, Weapon { name, min_damage: get("DamageMin"), max_damage: get("DamageMax"), attack_ms: get("Speed") }));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.name.cmp(&b.1.name)));
    Ok(out.into_iter().map(|(_, w)| w).collect())
}

/// Held-weapon model of a `BaseItemTypes` row, through its `ItemVisualIdentity`.
pub fn held_model(idx: &Index, dir: &Path, schema: &Schema, row: usize) -> Option<String> {
    let bytes = idx.read_file(dir, "data/balance/baseitemtypes.datc64").ok()?;
    let table = Table::parse(&bytes).ok()?;
    let r = table.decode_row(schema.table("baseitemtypes")?, row).ok()?;
    let Some((_, Value::Ref(Some(v)))) = r.iter().find(|(k, _)| k == "ItemVisualIdentity") else { return None };
    let vb = idx.read_file(dir, "data/balance/itemvisualidentity.datc64").ok()?;
    let vt = Table::parse(&vb).ok()?;
    let vr = vt.decode_row(schema.table("itemvisualidentity")?, *v as usize).ok()?;
    vr.iter().find_map(|(k, x)| match x { Value::Str(s) if k == "AOFile2" && !s.is_empty() => Some(s.to_ascii_lowercase()), _ => None })
}

/// First stat of a `Mods` row looked up by Id: (affix name, stat id, lowest roll).
pub fn mod_stat(idx: &Index, dir: &Path, schema_dir: &Path, mod_id: &str) -> Result<(String, String, i64)> {
    let schema = Schema::load(schema_dir)?;
    let mb = idx.read_file(dir, "data/balance/mods.datc64")?;
    let mt = Table::parse(&mb)?;
    let mts = schema.table("mods").context("no schema for mods")?;
    let sb = idx.read_file(dir, "data/balance/stats.datc64")?;
    let st = Table::parse(&sb)?;
    let sts = schema.table("stats").context("no schema for stats")?;
    for i in 0..mt.rows {
        let row = mt.decode_row(mts, i)?;
        if !row.iter().any(|(k, v)| k == "Id" && matches!(v, Value::Str(s) if s.eq_ignore_ascii_case(mod_id))) {
            continue;
        }
        let name = row.iter().find_map(|(k, v)| match v { Value::Str(s) if k == "Name" => Some(s.clone()), _ => None }).unwrap_or_default();
        let value = row.iter().find_map(|(k, v)| match v { Value::Int(x) if k == "Stat1Value" => Some(*x), _ => None }).unwrap_or(0);
        let Some(Value::Ref(Some(stat))) = row.iter().find(|(k, _)| k == "Stat1").map(|(_, v)| v.clone()) else { anyhow::bail!("mod {mod_id} has no stat") };
        let srow = st.decode_row(sts, stat as usize)?;
        let stat_id = srow.iter().find_map(|(k, v)| match v { Value::Str(s) if k == "Id" => Some(s.clone()), _ => None }).unwrap_or_default();
        return Ok((name, stat_id, value));
    }
    anyhow::bail!("no mod {mod_id}")
}
