# Porting plan

Offline Rust reimplementation of Path of Exile 2 that reads the user's own install. Static analysis only: no GGG servers, no anti-cheat bypass. Server behaviour (AI, loot, damage formulas, zone seeds) is not in client files, so it is reconstructed, never copied.

## Done
- Bundles2 / dat readers, models, materials, skeletons, animations, characters, rooms (`.arm`), tiles (`.tdt` parsed).
- Play sandbox: isometric camera (estimated constants), WASD, collision with tall objects, attack, dodge roll, one chasing monster.
- Numbers from tables: player life/damage/swing speed/roll time (`Characters`, `GrantedEffects`), monster life/damage (`DefaultMonsterStats` x `MonsterVarieties`).

## Status notes (2026-10-29)
- Item 3 partly done: `BaseItemTypes` reads (first 217 bytes of the row), the class's starting weapon sets player damage and swing speed through `WeaponTypes`. Tab cycles weapons with drop level <= 1 (damage and swing speed from `WeaponTypes`); the model in hand is the class start weapon (`BaseItemTypes` -> `ItemVisualIdentity.AOFile2`) and Tab now swaps it: the start weapon plus up to 8 candidates are loaded once and only the one in hand is drawn. Items that are not held in the right hand (talisman, bow, claw) show nothing.
- Item 1 blocked on Ghidra: no camera constants in data files or client strings. Tried the strings `CameraUpdate` / `PreCameraUpdate`: they only name phases of the frame scheduler (`FUN_142120710`), not projection math. A real hit needs the camera class (e.g. via a vtable or the `fTanHalfFOV` shader constant's producer); `ProjectionTransform_Calc` / `_Init` are likewise only transform-stage names in a registry (`FUN_1400214c0`). Strings such as `kInputViewport*`, `viewport_size`, `inner_zoom`, `target_zoom` exist but were not followed. Third attempt (`viewport_size`, `target_zoom`, `inner_zoom`): the zoom strings are only entries of script-binding tables (referenced from data, no function), `viewport_size` sits in a renderer pass that uploads `inv_proj_matrix`; no fixed pitch/FOV constant surfaced. Camera zoom/pan in the client is driven by script (`target_zoom`), so the real game camera may be a script/data-driven rig rather than constants in code. `scratchpad` script `StrRefs.java` (slow) / `StrRefs2.java` (one pass) (not in git) lists and decompiles functions that reference a string.
- Item 4 findings: `Stats` decodes; `Mods` is now readable (all 16784 rows): every `StatNValue` is a (min, max) interval in this build and `Stat8Min/MaxHardmode` are gone; fixed in `schema_overrides.txt`. `GameConstants` and `ExperienceLevels` have no community schema; `GameConstants` is 16-byte rows (name, value, divisor) and holds ailment, stun and monster-defence constants but no attribute-to-life ratios.

- Item 4 progress: the player template `metadata/characters/character.ot` holds `life_per_level = 12`, `mana_per_level = 4`, base evasion 7; player life = `Characters.BaseMaxLife` + 12 x level (28 at level 1; `POE2_LEVEL`). Attribute-to-life ratios are still not found.

- Item 5 progress: `--world <zone>.tgr` works in play mode (crude preview: room nodes as flat ground, no gaps filled, no real generator); the player starts on the geometry nearest the zone centre (edge of a room in `intro_1.tgr`). Nothing keeps the player inside the rooms.

## Next, in order
1. Camera constants from the client (Ghidra), replacing the estimates.
2. `.tdt` grids: meaning of the 23x23 node grid and the tile-wide grid; connect heights and walkability to play mode.
3. Base items and equipment from `BaseItemTypes` / `ItemClasses` (needs the schema fix for the mismatched tables); equipped weapon changes damage and attack speed.
4. Attributes and the stat system (`Stats`, `Mods`), then skills from `GrantedEffects*`.
5. Zone generation from `.tgr` graphs (reconstruction, not the real generator).
6. Monster AI and loot as documented, data-driven approximations.

## Out of scope
Anything that needs a GGG server or modifies the official client.
