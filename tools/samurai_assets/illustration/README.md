# Legendary weapons from the illustrations

Kusanagi and Muramasa v4 are cut from the user's illustrations (`Kusanagi.png`
and `Muramasa Transformed.png` in art.zip, not stored here). Run from a folder
holding those two files under `artzip/`:

```powershell
py cutout.py cut      # cut-outs: <name>-cut.png (as drawn) and <name>-flat.png (hilt left, edge below)
py native.py cut native   # game-size sources: <name>-v4.png and <name>-v4.json (grip, size)
```

Copy `native/<name>-v4.png` to `artwork/source/legendary/`, save the cut-outs
(scaled to 1024 px) as `<name>-v4-illustration.png` there, set `rect`,
`downsample` and `source_grip` in `legendary_weapons.json` from the JSON, then
build with `compile_weapons.py`. `mura_outline.py` is Muramasa's traced outline
and tassel in illustration pixels; Kusanagi's handle outline is in `cutout.py`.
