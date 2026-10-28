# HD-2D rendering

The goal is the *Octopath Traveler* / *Triangle Strategy* presentation: RO's 2D sprites
standing in a lit 3D diorama, with soft directional shadows, bloom, a tilt-shift blur and
storybook colour grading. The original look remains one toggle away.

## Frame pipeline

```
 sprites ──► sun shadow pass ──► shadow map (2048², depth)
                                      │
 ground, models, water, sprites, effects ──► HDR scene (Rgba16Float, MSAA resolved)
                                      │  (scene.wgsl samples the shadow map)
                                      ▼
 post chain:  bright pass ½ ─► ¼ ─► blur H ─► blur V ────────── bloom
              downsample ½ ─► ¼ ─► blur H ─► blur V ────────── DoF source
              composite: tilt-shift mix ─► + bloom ─► exposure / warmth /
                         saturation / contrast ─► highlight shoulder ─► vignette
                                      ▼
                                 swapchain ──► egui UI on top
```

| Piece | File | Notes |
|---|---|---|
| HDR target and post chain | `crates/ro-render/src/post.rs`, `post.wgsl` | Fullscreen-triangle passes; quarter-resolution blurs keep the cost low |
| Sun shadows for sprites | `shadow.rs`, `shadow.wgsl` | Sprites only. RO maps already bake building shadows into lightmaps, so static geometry does not cast |
| Shadow receiving | `scene.wgsl` (`sprite_shadow`) | 5×5 PCF with a comparison sampler; strength and softness from globals |
| Look settings | `PostSettings` (renderer), `settings::Visual` (client) | Saved in `settings.toml`, edited live in Settings → Visual |

### Sprite shadows

Each sprite layer is drawn into the shadow map as an upright card at the actor's feet,
alpha-tested at 0.5, so the shadow keeps the sprite's silhouette. The card faces the sun,
not the camera. A camera-facing card seen from the side collapses into a thin line, while a
sun-facing card throws its full outline. That matches how HD-2D games cast character shadows.
The round blob shadow under each actor is skipped as a caster but still drawn, as a contact
shadow.

The sun keeps the map's bearing (RSW longitude) but its height comes from
`sun_elevation` (35° by default). RO's own light is often nearly vertical, which hides
shadows under the sprite.

### Tilt-shift depth of field

There is no depth read-back, because MSAA depth is not sampleable. Instead the composite
blends toward a blurred copy by screen height, sharp in a band around the player (the camera
keeps them centred). For a top-down diorama this is the miniature look itself, and it costs
two quarter-resolution blurs.

### Tone

Shaders still produce the client's familiar gamma-tuned colours, so with everything neutral
the image matches the classic look. Grading is applied on top. A highlight shoulder rolls
off values above 0.8, so bloom and bright effects don't clip.

## Controls

Settings → Visual. The same fields are in `settings.toml` under `[visual]`.

| Control | Default | Effect |
|---|---|---|
| HD-2D lighting and effects | on | Off gives the classic look: plain copy, no shadows |
| Bloom | 0.22 | Glow strength (threshold 0.92 in `PostSettings`) |
| Depth of field | 0.85 | Tilt-shift blur toward the top and bottom of the screen |
| Shadows | 0.70 | How dark sprite shadows get |
| Sun height | 35° | Lower values give longer shadows |
| Vignette | 0.45 | Darkened corners |
| Warmth | 0.35 | Warm highlights, cool shadows (−1 to 1) |
| Saturation / Contrast | 1.10 / 1.12 | Grading around mid grey |

`PostSettings` also holds `bloom_threshold`, `dof_band`, `exposure` and
`shadow_softness` for finer tuning.

## Roadmap, in order of visual impact

1. **Sprite lighting.** Tint sprites by the map's ambient and diffuse light and by the
   shadow map at their feet, so characters darken under trees and awnings. Add a rim light
   from the sun direction for the "lit paper cut-out" edge.
2. **Point lights.** RSW maps list light sources (torches, lamps). Upload the nearest
   N (~32) as a storage buffer, add their falloff to ground, models and sprites, and give
   bright ones a small additive glow sprite. Pairs with bloom.
3. **Fog and light shafts.** Height fog toward the horizon edges (tinted by map
   ambient), plus a screen-space radial blur of the bloom bright pass from the sun's
   projected position for god rays.
4. **Ambient occlusion.** Bake per-vertex AO for RSM models at load (hemisphere rays
   against the GAT/model heights), or add a half-resolution SSAO pass once depth is
   resolved to a sampleable texture.
5. **Depth-aware DoF.** Resolve depth (or write linear depth into a second target) and
   blend the tilt-shift mask with real distance from the focal plane, so tall
   foreground objects blur correctly.
6. **Pixel stability.** Snap sprite anchors to the screen pixel grid at integer zooms,
   and keep sprite sampling nearest-neighbour when the camera is close.
