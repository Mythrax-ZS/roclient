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

## Lighting, atmosphere and stability

All six roadmap items are implemented:

1. **Sprite lighting** (`sprite.wgsl`). Each sprite takes the map light at its feet
   relative to full sun, so it keeps its colours in the open and darkens in shadow. Feet
   shadow comes from the shadow map, sampled a little toward the sun so a sprite never shades
   itself. Nearby light sources add to it, and a sun-coloured rim lights the edges whose
   neighbour toward the sun is transparent.
2. **Point lights**. RSW light sources (colour floats, range; position centred like models,
   y flipped) go to the renderer. The 32 nearest the camera ride in the globals and light
   ground and models (half-wrapped, smooth falloff) and sprites.
3. **Fog and god rays** (`post.wgsl`). Fog: linear distance from the depth-info target
   blends toward a pale sky tinted by the map ambient, from 1.15x to 2.6x the camera
   distance. God rays: the bloom bright pass is marched toward the sun's screen
   position (24 taps with decay) and added in the composite.
4. **Ambient occlusion** (`fs_ao`). A 12-tap spiral over linear depth at half
   resolution; nearby neighbours closer to the eye occlude, with range limits against
   halos. Sprites neither cast nor receive it.
5. **Depth-aware DoF**. Every world and sprite pixel writes `(distance, is_sprite)` to an
   `Rg16Float` target alongside colour, resolved from MSAA. The composite blur is the
   larger of the tilt-shift band and real distance from the focal plane (the player).
6. **Pixel stability**. Sprite anchors snap to whole screen pixels (`params.w`), so
   sprites don't shimmer as the camera glides.

Settings → Visual adds sliders for sprite lighting, rim light, light sources, fog,
god rays and ambient occlusion, plus a pixel-snapping toggle (`PostSettings` fields of
the same names).

### Ideas beyond this

- Per-light shadow maps for the brightest torches.
- Light flicker on fire-coloured sources, and small additive glow sprites on light positions.
- Baked per-vertex AO for models (cheaper and steadier than screen-space AO).
- Nearest-neighbour sprite sampling at high zoom.
