# Porting plan

Offline Rust reimplementation of Path of Exile 2 that reads the user's own install. Static analysis only: no GGG servers, no anti-cheat bypass. Server behaviour (AI, loot, damage formulas, zone seeds) is not in client files, so it is reconstructed, never copied.

## Done
- Bundles2 / dat readers, models, materials, skeletons, animations, characters, rooms (`.arm`), tiles (`.tdt` parsed).
- Play sandbox: isometric camera (estimated constants), WASD, collision with tall objects, attack, dodge roll, one chasing monster.
- Numbers from tables: player life/damage/swing speed/roll time (`Characters`, `GrantedEffects`), monster life/damage (`DefaultMonsterStats` x `MonsterVarieties`).

## Next, in order
1. Camera constants from the client (Ghidra), replacing the estimates.
2. `.tdt` grids: meaning of the 23x23 node grid and the tile-wide grid; connect heights and walkability to play mode.
3. Base items and equipment from `BaseItemTypes` / `ItemClasses` (needs the schema fix for the mismatched tables); equipped weapon changes damage and attack speed.
4. Attributes and the stat system (`Stats`, `Mods`), then skills from `GrantedEffects*`.
5. Zone generation from `.tgr` graphs (reconstruction, not the real generator).
6. Monster AI and loot as documented, data-driven approximations.

## Out of scope
Anything that needs a GGG server or modifies the official client.
