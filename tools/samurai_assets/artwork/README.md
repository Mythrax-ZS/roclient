# Samurai RO artwork revision

The `source` directory contains selected generated PNG sheets without painted
postprocessing. `build.json` records every sampled rectangle, fixed character
origin, sheet calibration, timeline and weapon grip. Build from the client root:

```powershell
py tools/samurai_assets/compile.py tools/samurai_assets/artwork/build.json --output ../rathena/generated/custom_jobs/samurai_ro_v2/compiled
```

The source art was generated for this project on 2026-10-29. The revised design
uses compact RO proportions, muted slate cloth and burgundy accents.
The full-body images include head and hair. The katana remains a
separate layer; `source/katana.spr` preserves the existing generated katana art.
Its visually measured handle grip is at pixel (10, 4), left of the gold guard.

Five authored directions are S, SW, W, NW and N. NE, E and SE use ACT mirroring
of NW, W and SW. The clothing was designed to tolerate this mirrored reuse.
Sheet calibration is uniform for each complete source sheet and applied in one
sampling operation together with the shared divisor. Individual poses retain
their proportions and cloth motion; they are not scaled from silhouette bounds.

Idle and ready origins were authored from each stance and ground plane. Walk
origins use a stable torso/pelvis registration so the advancing foot does not
pull the body sideways. Weapon grips and rotations are authored metadata,
including poses whose hands are partly hidden by the body when facing away.

Stock RO images used for visual comparison remain outside this source directory
and are not part of the distributed overlay. Generated source PNGs, metadata,
compiler scripts and compiled SPR/ACT files provide the reproducible asset path.

The complete build contains all 13 standard groups: 104 directional actions,
304 timeline frames and 119 unique RGBA body images. Body and katana timelines
have matching frame counts and delays. The three attack groups explicitly share
the same authored six-pose cycle; walk uses eight poses per direction. Forward
walk uses one selected contact pose from the supplemental sheet, with the exact
ordering recorded in `walk_audit.json` and `build.json`.

`walk_audit.json`, `utility_audit.json` and `source/attack/attack_audit.json`
record the authored roots and registrations. `prompts_root.json` and the
per-sheet prompt/provenance files preserve the generation instructions. The
compiler refuses incomplete group sets. Runtime overlay installation is a
separate step; the overlay provenance records the installed binary hashes.
