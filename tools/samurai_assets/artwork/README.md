# Samurai black and red artwork

This package uses the user's selected character design: a charcoal kimono with
red piping and chest knots, a red split coat, long black ponytail, black boots
with red trim, and a gold belt loop. `source/idle_ready.png` is the selected
reference copied unchanged. The movement and action sheets were restyled to
match that design with imagegen.

The full-body images include head and hair. The katana remains a separate
layer. `source/katana.png` is the generated curved silver blade with a red and
black wrapped hilt. Its authored rectangle is sampled once into a 72-by-10
pixel canvas, with 58 visible pixels of length and a main blade about 5 pixels
wide. The exposed blade is roughly 25% shorter than the first version, and its
width increased from about 3 pixels. The grip is at (14, 5)
in that sampled canvas. Every armed ACT pose uses scale 1.0; the former 0.75
scale made the old thin blade disappear into a needle-like line in-game.
`source/katana.prompt.json` records the original generation; the exact edits
and sampling parameters are in `source/katana.refinement.json`. The final
shape was checked in the prior revision at 2000-by-1125 resolution and camera
distances 300 and 420. The current attack orientation revision changes its ACT,
while preserving the starter katana's source PNG and size.

The build preserves the accepted animation contract: 104 directional actions,
304 timeline frames, eight walking poses at 100 ms each, and paired body/weapon
timelines. Five source directions are S, SW, W, NW and N; NE, E and SE use ACT
mirroring. The three attack groups share a six-pose cycle. The forward walk
uses selected passing/contact repair poses in its recorded chronology.

The attack revision corrects the cutting-edge side for SW, W, NW and N, and
their reflected directions. The source blade points right with its edge below;
horizontal mirroring plus a 180-degree rotation reverses the edge side without
reversing the tip direction. Five winding/overhead poses also align the blade
with the stacked hands. S retains its clockwise cutting orientation. The compact
review record is `weapon_pose_audit.json`; all grip coordinates and body art stay
unchanged. Runtime captures from earlier revisions do not validate these changes.

`build.json` records explicit source rectangles, body origins, uniform sheet
calibration and weapon grips. Whole-sheet calibration and the shared divisor
are applied in one nearest-neighbor sample. Individual frames are never sized
or centered from their silhouettes. The source PNGs remain unchanged.

Build from the client repository:

```powershell
py tools/samurai_assets/compile.py tools/samurai_assets/artwork/build.json --output target/samurai-review
```

The master, walk, utility and attack audits record registration decisions.
`prompts_root.json` and the action and katana prompt files retain generation instructions
from the built-in image-generation tool. The compiled package supplies decoded
contact sheets, an animated HTML preview, binary checksums and source-edge
diagnostics. Stock RO comparison images are not included in this source package.

Source and decoded contact reviews cover all eight directions, opposite-leg
walk phases, hand grips, rear-facing weapon occlusion and source clipping.
The complete package has 119 body images. All 104 action frame counts and delays
were checked against the previous accepted animation; they are unchanged.
`selection_manifest.json` lists the selected inputs and runtime checksums.

## Kusanagi and Muramasa source revision

The selected legendary weapon sources are `source/legendary/kusanagi-v4.png`
and `source/legendary/muramasa-v4.png`, made from the user's own illustrations
(`Kusanagi.png` and `Muramasa Transformed.png` in art.zip) rather than generated.
`tools/samurai_assets/illustration` holds the steps: `cutout.py` cuts each sword
out of its background (Kusanagi by its glow plus a traced handle; Muramasa by the
outline and tassel traced in `mura_outline.py`) and lays it flat with the cutting
edge below; `native.py` scales pommel to tip to 58 game pixels, thickens the
slender painted blades in height (Kusanagi x1.45, Muramasa x1.35) so they read in
game, adds a warm dark rim, and writes the block-upscaled PNGs sampled here. Both
keep the illustrations' gentle katana curve; Muramasa keeps its tassel past the
pommel. The `*-v4-illustration.png` cut-outs are the item description pictures
(`illustration` in `legendary_weapons.json`). Both use scale 1.0 and the shared
corrected attack and ready-pose metadata. The v3, v2 and unversioned PNGs and
prompts remain historical references; they are no longer selected runtime inputs.

`legendary_weapons.json` specifies sampling rectangles and grip coordinates.
`source/legendary/sampling_review.json` records the native-size review. Rebuild
the weapons, inventory/collection art and floor sprites together:

```powershell
py tools/samurai_assets/compile_weapons.py tools/samurai_assets/artwork/build.json tools/samurai_assets/artwork/legendary_weapons.json --output target/legendary-weapons-v4
```

The compiler writes a staging overlay and decoded previews under that output
directory. [Legendary provenance](../../../overlays/samurai/legendary_provenance.json)
distinguishes current validation from checks performed before this revision.
