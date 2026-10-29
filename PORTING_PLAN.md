# Porting plan

Offline Rust reimplementation of Path of Exile 2 that reads the user's own install. Static analysis only: no GGG servers, no anti-cheat bypass. Server behaviour (AI, loot, damage formulas, zone seeds) is not in client files, so it is reconstructed, never copied.

## Done
- Bundles2 / dat readers, models, materials, skeletons, animations, characters, rooms (`.arm`), tiles (`.tdt` parsed).
- Play sandbox: isometric camera (estimated constants), WASD, collision with tall objects, attack, dodge roll, one chasing monster.
- Numbers from tables: player life/damage/swing speed/roll time (`Characters`, `GrantedEffects`), monster life/damage (`DefaultMonsterStats` x `MonsterVarieties`).

## Status notes (2026-10-29)
- Item 3 partly done: `BaseItemTypes` reads (first 217 bytes of the row), the class's starting weapon sets player damage and swing speed through `WeaponTypes`. Swapping weapons in play mode is not done.
- Item 1 blocked on Ghidra: no camera constants in data files or client strings.
- Item 4 findings: `Stats` decodes; `Mods` is now readable (all 16784 rows): every `StatNValue` is a (min, max) interval in this build and `Stat8Min/MaxHardmode` are gone; fixed in `schema_overrides.txt`. `GameConstants` and `ExperienceLevels` have no community schema; `GameConstants` is 16-byte rows (name, value, divisor) and holds ailment, stun and monster-defence constants but no attribute-to-life ratios.

- Item 4 progress: the player template `metadata/characters/character.ot` holds `life_per_level = 12`, `mana_per_level = 4`, base evasion 7; player life = `Characters.BaseMaxLife` + 12 x level (28 at level 1; `POE2_LEVEL`). Attribute-to-life ratios are still not found.

## Next, in order
1. Camera constants from the client (Ghidra), replacing the estimates.
2. `.tdt` grids: meaning of the 23x23 node grid and the tile-wide grid; connect heights and walkability to play mode.
3. Base items and equipment from `BaseItemTypes` / `ItemClasses` (needs the schema fix for the mismatched tables); equipped weapon changes damage and attack speed.
4. Attributes and the stat system (`Stats`, `Mods`), then skills from `GrantedEffects*`.
5. Zone generation from `.tgr` graphs (reconstruction, not the real generator).
6. Monster AI and loot as documented, data-driven approximations.

## Out of scope
Anything that needs a GGG server or modifies the official client.
