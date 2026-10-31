# Texture upscaling

Environment and interface textures can be upscaled for a cleaner HD look. **Character and
effect sprites are never touched**: they keep their original pixels, size and filtering.

Code: `crates/ro-render/src/upscale.rs` (settings, resampler, cache), the world shader's
`catmull_rom` in `crates/ro-render/src/scene.wgsl`, and `crates/ro-client/src/uitex.rs`
(interface textures).

## 1. What gets upscaled, and how sprites are kept out

| Asset | Class | Load point | Upscaled |
|---|---|---|---|
| GND ground textures, RSW water frames | `Ground` | `load_map` (main.rs) | yes |
| RSM model textures | `Model` | `load_map` (main.rs) | yes |
| Item icons, collection pictures | `ItemIcon` | `load_texture` → `uitex::load` | yes |
| Window art, buttons, minimaps, status icons, card art, loading screens | `Interface` | `load_texture` / splash → `uitex::load` | yes |
| Player / monster / NPC / pet sprites (SPR/ACT) | none | `sprite::Sheet::upload` | **never** |
| Sprite effects (`sprite\이팩트`), emotes, cursors, character previews | none | sprite sheets / `preview::compose` | **never** |
| STR effect textures, NPC cut-ins, guild emblems | none | their own loaders | no |

Sprites are excluded by construction, not by a setting:

1. **Separate code path.** Sprite sheets are built and uploaded by `sprite::Sheet::upload`
   and drawn by `sprite.wgsl`, with their own sampler. That path never calls the upscaler,
   and the run-time filter lives only in the world shader (`scene.wgsl`).
2. **No sprite class.** `AssetClass` has no sprite variant, so a sprite cannot be requested.
3. **Path guard.** `Upscaler::apply` refuses anything under `data\sprite\` (or `.spr` /
   `.act`) whatever class it was given. In debug builds it asserts.
4. **Test.** `upscale::tests::sprites_are_never_upscaled` checks the guard.

To add a new upscaled asset type, add an `AssetClass` variant and its switch, then call
`Upscaler::apply` / `apply_all` at its load point. Never do this from sprite code.

## 2. The two stages

```
decode (VFS) ─► [load-time resample, CPU] ─► [disk cache] ─► mip chain ─► GPU
                                                                          │
world shader: magnified? ─► 9-tap Catmull-Rom : else trilinear + 16x aniso ◄┘
```

* **Load time** (Balanced, High). Each texture is resampled once with a separable filter in
  premultiplied alpha. RO's magenta colour key becomes transparent black, so straight
  filtering would darken every cut-out edge. Borders clamp to the edge, like the sampler.
  Balanced uses Catmull-Rom; High uses Lanczos-3, clamped to the two nearest source texels
  (anti-ringing) so hard edges don't halo. A map's textures are resampled in parallel across
  the CPU cores. The usual mip chain is built from the result, so distant surfaces are
  unchanged.
* **Run time** (every quality except Off). The world shader works out each pixel's texel
  footprint from its UV derivatives. Where a texture is magnified (a texel covers more than
  one pixel), it filters with 9-tap Catmull-Rom instead of bilinear. Elsewhere it samples
  normally, with trilinear and 16x anisotropic filtering. There's no extra memory, and the
  cost is 9 taps only on magnified pixels.
* **Interface textures** are resampled at load like the world. They then draw with linear
  filtering and mipmaps, but at their original size: `uitex` records each texture's
  stored-to-art ratio, and `ui_size()` returns the art size. Windows that place art by
  pixel coordinates therefore don't move. **Always size interface textures with
  `ui_size()`, never egui's `size_vec2()`.** Textures that weren't upscaled return the same
  value either way.

**Other options considered.** Lanczos-3 is the High preset. Real-time neural upscalers
(ESRGAN-type) need an ML runtime and are far too heavy per frame. Temporal upscaling
targets the whole frame, not textures. Offline neural upscaling works best as an HD texture
pack: the VFS `overlays` already let a folder replace any texture at any resolution, and
UVs are normalised, so an ESRGAN-processed pack drops in without code changes. Set
`quality = "fast"` for such a pack, so it is filtered at run time but not resampled again.
`tools/texpack` builds one with realesrgan-ncnn-vulkan, with seamless wrap padding for tiling
ground and crisp colour-keyed cut-outs. Prontera (217 textures) takes about 25 s on an RTX 5070
Ti and about 250 MB as PNG. Compared with Lanczos-3, it gives cleaner, sharper mortar and
edges but smooths away the painted grain.

## 3. Configuration (`config/settings.toml`)

```toml
[upscale]
quality = "balanced"   # off | fast | balanced | high
factor = 2.0           # 1.5 | 2 | 3 | 4 (load-time; balanced and high only)
max_size = 2048        # cap on the longer side of an upscaled texture
ground = true          # per-category switches; sprites have none
models = true
item_icons = true
interface = true
exclude = []           # path substrings to leave alone, e.g. ["prontera\\", "lighthouse"]
cache = false          # keep results in cache/upscale (worth it at 3x and 4x)
```

| Preset | Load-time resample | Run-time filter | Extra GPU memory |
|---|---|---|---|
| `off` | none | bilinear (the original) | none |
| `fast` | none | Catmull-Rom when magnified | none |
| `balanced` (default) | Catmull-Rom × factor | Catmull-Rom when magnified | about factor² × original |
| `high` | Lanczos-3 × factor, anti-ringing | Catmull-Rom when magnified | about factor² × original |

In game: Settings → Textures → **Upscaling** and **Upscale factor**. The run-time filter
switches instantly. Load-time changes apply to the next map entered and to interface art
loaded afterwards.

`exclude` gives per-texture control. Matching is a case-insensitive substring of the path
under `data\texture\`, or of the full path for interface art.

## 4. Performance (Prontera, 217 textures, 16 worker threads, release build)

| Setting | Resample time | From cache | Cache on disk |
|---|---|---|---|
| Balanced 2x | 135 ms | 45 ms | about 100 MB |
| High 4x | 508 ms | 175 ms | about 350 MB |

* GPU memory for world textures grows by about factor² (2x ≈ 4×, 4x ≈ 16×, capped by
  `max_size`). 2x is the sensible default for mid-range GPUs; keep 3x and 4x for cards with
  plenty of VRAM.
* The run-time filter adds 8 taps only on magnified pixels. At the default camera, most
  ground is drawn about 1:1 or minified, so the cost is small.
* The cache trades disk for load time. At 2x the resample is already about as fast as
  reading the cache back, so it is off by default.
