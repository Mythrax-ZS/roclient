# Samurai appearance assets

This overlay supplies the male Samurai body and its separately equipped katanas.
The game job is **Samurai** (4500); **Ember Ronin** remains the asset filename.

## Rendering contract

- The body includes the head and ponytail. Do not add stock heads, hair,
  headgear, garment or shield layers. Its colors are authored into the sprite.
- The body and each weapon ACT contain 104 actions. Directions are S, SW, W, NW, N, NE, E, SE;
  the action index is `group * 8 + direction`.
- Use human player actions: idle 0, walk 1, sit 2, pickup 3, ready 4,
  attack 5, hurt 6, freeze 7, death 8, freeze 9, attack 10/11, cast 12.
- Walking has eight frames per direction at 100 ms per frame. Continuous movement
  retains its animation phase across tiles and turns.
- NE/E/SE mirror the authored NW/W/SW views, including layer positions and weapon
  rotation. Mirroring also reverses costume details.
- Attack groups 5, 10 and 11 correct the cutting-edge side for SW/W/NW/N and
  their reflected directions, while S keeps its clockwise cutting orientation.
  Five winding/overhead poses also align the blade with the hand axis. The shared
  metadata applies to the starter katana, Kusanagi and Muramasa.
- The weapon follows the body's action, frame index and delay. Its offsets are
  relative to the actor's ground origin; do not apply a head-anchor offset.
- Katana layers are present in ready and attack groups 4, 5, 10 and 11.
  All other weapon frames have empty layers.
- The katana uses native scale 1.0 in every armed pose. Its curved blade and
  wrapped hilt occupy 58 pixels of the 72-by-10 source canvas, with a main blade
  about 5 pixels wide. The blade was shortened and widened for gameplay zoom; do not
  restore the old 0.75 scale, which made the thin blade hard to read in-game.
- Equipped swords with view 2, 3 or 49 use the starter katana. Custom views 9001
  and 9002 select Kusanagi and Muramasa. Dual wielding is a
  server mechanic; separate offhand and katar artwork are not supplied.
- All pairs use SPR 2.1 truecolor RGBA and ACT 2.5. Weapon pairs must match the
  body's action layout and authored grip positions.

## Reference design revision

The artwork matches the reference selected by the user on 2026-10-30: charcoal
wrap clothing with bright red piping and cord knots, a red rope belt, long red
split coat panels, black/red boots and a long dark ponytail. The exact selected
image supplies idle and ready poses; the other actions use that same design.
The previous corrected walk poses provide the movement reference.

Walk cycles use explicit pose selection and fixed anatomical ground origins.
One scale calibration applies to each complete source sheet. Individual frames
are never resized to their silhouette or centered on the leading foot.
The south walk combines selected contact and passing poses from two generated
sheets; its exact sequence is recorded in the build metadata.

The same design is used for idle, ready, walking, sitting, pickup, hurt, casting,
falling and attacking. The three attack groups share one six-frame swing per
authored direction. Female art, independently selectable heads/hair/headgear,
alternate weapon artwork and custom skill effects remain outside this package.

## Sources and rebuilding

The checked-in source artwork, placement metadata and exact generation prompts
are in [tools/samurai_assets/artwork](../../tools/samurai_assets/artwork/README.md).
The [compiler](../../tools/samurai_assets/README.md) samples and packs those
sources; it does not draw poses or infer anatomy.

From the client repository:

```powershell
py tools/samurai_assets/compile.py tools/samurai_assets/artwork/build.json --output target/samurai-review
py -m unittest discover -s tools/samurai_assets -p 'test_*.py'
```

Open `target/samurai-review/preview.html` to inspect all directions, step through
frames and toggle the katana. Contact sheets and the validation report are
decoded from the output binaries. Installation is a separate copy of both
SPR/ACT pairs into `data/sprite/custom/samurai/`.

[provenance.json](provenance.json) records the runtime file hashes and source
locations. The artwork was generated with the built-in image-generation tool.
Source images retain their generated alpha; nearest-neighbor sampling and ACT
placement are reproducible from the metadata.

`System/samurai_iteminfo.lua` supplies item 900000, **Samurai Initiate's Katana**.
It references the installed game's stock Katana item/collection icons; those
icons are not redistributed in this overlay.

## Kusanagi and Muramasa

| Item ID | Name | Client view | Role |
| --- | --- | --- | --- |
| 900001 | Kusanagi | 9001 | Divinity: precision, critical strikes and efficient SP use |
| 900002 | Muramasa | 9002 | Cursed: stronger attacks and Bleeding, with reduced Max HP |

Both are Samurai-only one-handed swords, weapon level 4, one slot, required
base level 99, refineable and Neutral element. The server owns their stats and
effects; see [the server specification](../../../rathena/doc/custom_jobs/samurai.md).
Their gameplay design uses the Divinity/Cursed themes from the earlier shared
ChatGPT conversation. The private Discord message was not accessible; no extra
origin story is asserted. The user's gold/blue and red/black images define
their visual identities.

The overlay includes separate equipped SPR/ACT pairs, 32-pixel inventory icons,
128-pixel collection art and floor-drop sprites. Either weapon can be equipped
in either hand mechanically; the current client displays the main-hand weapon.
The existing stock Muramasa (1164) remains a separate item.

The selected weapon art is the `*-v4.png` pair in the source directory below,
cut out of the user's Kusanagi and Muramasa illustrations and scaled to 58 game
pixels from pommel to tip, with the blades thickened so they read at gameplay
zoom. The item description pictures are the cut-out illustrations themselves.
The v3, v2 and unversioned PNGs and prompts are retained only as historical
references. Weapon sizes and ACT scales remain independent of the
inventory/collection image sizes.

Rebuild their art from the client root:

```powershell
py tools/samurai_assets/compile_weapons.py tools/samurai_assets/artwork/build.json tools/samurai_assets/artwork/legendary_weapons.json --output target/legendary-weapons-v4
```

The staging overlay is `target/legendary-weapons-v4/overlay`; each weapon directory
also contains decoded contacts and an animated preview. The generated PNGs and
exact built-in imagegen prompts are in `tools/samurai_assets/artwork/source/legendary`.
[legendary_provenance.json](legendary_provenance.json) records their source and
runtime hashes and separates prior-revision checks from current validation.
Restart the client after installing updated sprite files. Item stats are
unchanged by this visual revision; `@item 900001 1` and `@item 900002 1` create
the existing weapons for testing.
