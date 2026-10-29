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
shape was checked at 2000-by-1125 resolution and camera distances 300 and 420.

The build preserves the accepted animation contract: 104 directional actions,
304 timeline frames, eight walking poses at 100 ms each, and paired body/weapon
timelines. Five source directions are S, SW, W, NW and N; NE, E and SE use ACT
mirroring. The three attack groups share a six-pose cycle. The forward walk
uses selected passing/contact repair poses in its recorded chronology.

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
