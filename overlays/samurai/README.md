# Samurai appearance assets

This overlay supplies the male Samurai body and its separately equipped katana.
The game job is **Samurai** (4500); **Ember Ronin** remains the asset filename.

## Rendering contract

- The body includes the head and ponytail. Do not add stock heads, hair,
  headgear, garment or shield layers. Its colors are authored into the sprite.
- Both ACT files contain 104 actions. Directions are S, SW, W, NW, N, NE, E, SE;
  the action index is `group * 8 + direction`.
- Use human player actions: idle 0, walk 1, sit 2, pickup 3, ready 4,
  attack 5, hurt 6, freeze 7, death 8, freeze 9, attack 10/11, cast 12.
- Walking has eight frames per direction at 100 ms per frame. Continuous movement
  retains its animation phase across tiles and turns.
- NE/E/SE mirror the authored NW/W/SW views, including layer positions and weapon
  rotation. Mirroring also reverses costume details.
- The weapon follows the body's action, frame index and delay. Its offsets are
  relative to the actor's ground origin; do not apply a head-anchor offset.
- Katana layers are present in ready and attack groups 4, 5, 10 and 11.
  All other weapon frames have empty layers.
- Equipped swords with view 2, 3 or 49 use the matching katana. Dual wielding is a
  server mechanic; separate offhand and katar artwork are not supplied.
- Both pairs use SPR 2.1 truecolor RGBA and ACT 2.5. Keep the four files together
  when replacing an asset revision.

## RO style revision

The replacement artwork uses a smaller face, narrower silhouette, muted slate
cloth and burgundy accents, with stock RO player sprites used as anatomy and
motion references. No extracted stock sprites are distributed here.

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
