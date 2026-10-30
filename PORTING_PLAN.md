# Porting plan

Offline Rust reimplementation of Path of Exile 2 that reads the user's own install. Static analysis only: no GGG servers, no anti-cheat bypass. Server behaviour (AI, loot, damage formulas, zone seeds) is not in client files, so it is reconstructed, never copied.

## Done
- Bundles2 / dat readers, models, materials, skeletons, animations, characters, rooms (`.arm`), tiles (`.tdt` parsed).
- Play sandbox: isometric camera (estimated constants), WASD, collision with tall objects, attack, dodge roll, one chasing monster.
- Numbers from tables: player life/damage/swing speed/roll time (`Characters`, `GrantedEffects`), monster life/damage (`DefaultMonsterStats` x `MonsterVarieties`).

## Status notes (2026-10-29)
- Item 3 partly done: `BaseItemTypes` reads (first 217 bytes of the row), the class's starting weapon sets player damage and swing speed through `WeaponTypes`. Tab cycles weapons with drop level <= 1 (damage and swing speed from `WeaponTypes`); the model in hand is the class start weapon (`BaseItemTypes` -> `ItemVisualIdentity.AOFile2`) and Tab now swaps it: the start weapon plus up to 8 candidates are loaded once and only the one in hand is drawn. Items that are not held in the right hand (talisman, bow, claw) show nothing.
- Item 1 blocked on Ghidra: no camera constants in data files or client strings. Tried the strings `CameraUpdate` / `PreCameraUpdate`: they only name phases of the frame scheduler (`FUN_142120710`), not projection math. A real hit needs the camera class (e.g. via a vtable or the `fTanHalfFOV` shader constant's producer); `ProjectionTransform_Calc` / `_Init` are likewise only transform-stage names in a registry (`FUN_1400214c0`). Strings such as `kInputViewport*`, `viewport_size`, `inner_zoom`, `target_zoom` exist but were not followed. Third attempt (`viewport_size`, `target_zoom`, `inner_zoom`): the zoom strings are only entries of script-binding tables (referenced from data, no function), `viewport_size` sits in a renderer pass that uploads `inv_proj_matrix`; no fixed pitch/FOV constant surfaced. Camera zoom/pan in the client is driven by script (`target_zoom`), so the real game camera may be a script/data-driven rig rather than constants in code. Fourth attempt: the script-binding table for `location, duration, zoom` (data at `0x1433d6758`) is a list of parameter names followed by wrapper pointers; its handler `FUN_14191be20` just writes the zoom into a Game component field (+0x1ac) after "Requires Game component" checks. No projection constants. Stopping the camera hunt here: the real camera needs the camera component's update code, which none of the strings reach. `scratchpad` script `StrRefs.java` (slow) / `StrRefs2.java` (one pass) (not in git) lists and decompiles functions that reference a string.
- Item 4 findings: `Stats` decodes; `Mods` is now readable (all 16784 rows): every `StatNValue` is a (min, max) interval in this build and `Stat8Min/MaxHardmode` are gone; fixed in `schema_overrides.txt`. `GameConstants` and `ExperienceLevels` have no community schema; `GameConstants` is 16-byte rows (name, value, divisor) and holds ailment, stun and monster-defence constants but no attribute-to-life ratios.

- Item 4 progress: the player template `metadata/characters/character.ot` holds `life_per_level = 12`, `mana_per_level = 4`, base evasion 7; player life = `Characters.BaseMaxLife` + 12 x level (28 at level 1; `POE2_LEVEL`). Attribute-to-life ratios are still not found.

- Item 5 progress: `--world <zone>.tgr` works in play mode (crude preview: room nodes as flat ground, no gaps filled, no real generator); the player starts on the geometry nearest the zone centre (edge of a room in `intro_1.tgr`). The player is kept on the zone's ground triangles (slides along edges; `POE2_FREE_ROAM=1` disables). Checked in a 20 s headless run: free roam ends off the ground, constrained ends on it.

- Item 6 check: `data/balance` has no drop-pool / loot-table files (only `DropEffects`, `ChanceableItemClasses`, treasure-hunter and auto-loot-pet tables), so item drop chances are not in the client data: loot rules live on the server.

- Item 6 approximation: every third kill equips the next weapon of the Tab list (invented rule, labelled "loot (invented rule)" in the status); verified in a 30 s headless run: kills at levels 1-3 gave the Changeling Talisman.

- Item 2 finding (2026-10-30): the per-sub-tile 23x23 node grids ARE a quantised height field, not hull/walkability data. Evidence: client strings `DecompressSubTileHullPlane()` (planes of 23 or 27 cells, decoded by `FUN_141d051c0`) pointed at the layout; adjacent nodes match along shared edges (node k's last row equals node k+3's first row in a 3x3 tile); assembling the nodes row-major per sub-tile (a tile with one node fewer than sub-tiles lacks the last) correlates 0.68-0.82 with the tile mesh's per-cell median z over four tiles, with slope -6.8..-7.8 model units per step (up is -z), so `tdt::HEIGHT_STEP` = 7.5 (estimate; correlation is limited because the mesh includes decor). `TdtFull::height_field()` assembles it. Not yet wired into play mode: the player still walks at z = 0.

## Next, in order
1. Camera constants from the client (Ghidra), replacing the estimates.
2. `.tdt` grids: meaning of the 23x23 node grid and the tile-wide grid; connect heights and walkability to play mode.
3. Base items and equipment from `BaseItemTypes` / `ItemClasses` (needs the schema fix for the mismatched tables); equipped weapon changes damage and attack speed.
4. Attributes and the stat system (`Stats`, `Mods`), then skills from `GrantedEffects*`.
5. Zone generation from `.tgr` graphs (reconstruction, not the real generator).
6. Monster AI and loot as documented, data-driven approximations.

## Out of scope
Anything that needs a GGG server or modifies the official client.

- Item 2 (heights in play mode): `.tdt` height fields are now kept per placed tile (`Mesh.heights`, `HeightPatch`), and the player and monsters are placed at `ground_z` (relative to the start; `POE2_FLAT=1` disables). Run on `gallows/act1/1_13/rooms/unique/exit_manor_01_nocrop.arm`: 28 height patches, steps 0..255 (most cells 0, 41, 64; the start lands on step 0). Not verified: that the figures follow terrain visually, HEIGHT_STEP (fitted), the grid origin at the slot centre, and the tile rotation sign. Most rooms in the data have no slots with a matching tile (0 patches there).
- Item 2 check: a 6 s walk (`POE2_DEBUG=1` prints `walk:` lines in the screenshot mode) across the same room reads ground z 0.0, 7.5 (one step) and back to 0.0 along a 3000-unit diagonal. So the sampler follows the field, but this path is almost flat; the screenshot was not compared against the real game, and the three inputs (HEIGHT_STEP, grid origin, rotation sign) stay unproven.
- Item 2, rotation sign (seam check, `POE2_DEBUG=1` prints `terrain: seam mismatch`): in `exit_manor_01_nocrop.arm` the mean height difference across tile seams is 111 model units with the default sign (-1) and 151 with `POE2_ROT_POS=1`, so -1 fits better, matching the earlier eye test. Both values are large (about 15 height steps), so the seams do not line up well; the grid origin or per-tile height offsets are probably still wrong. A weak result, not a proof.
