# Samurai appearance assets

This overlay contains the male Samurai body and its separately equipped katana.
The artwork was generated earlier for this project under the working name
**Ember Ronin**. The game job name is **Samurai**.

`data/sprite/custom/samurai/` contains the original, validated SPR/ACT pairs.
Each SPR embeds its default palette, so separate `.pal` files are unnecessary.
Keep each body/weapon pair together when replacing an asset revision.

## Rendering contract

- The body includes the head and hair. Do not add stock heads, hair, headgear,
  garment or shield layers to this appearance. Its palette is fixed.
- Both ACT files contain 104 actions and 376 timeline frames. Directions are
  S, SW, W, NW, N, NE, E, SE; action index is `group * 8 + direction`.
- Use ordinary human player actions: idle 0, walk 1, sit 2, pickup 3, ready 4,
  attack 5, hurt 6, freeze 7, death 8, freeze 9, attack 10/11, cast 12.
- The weapon uses the same action and frame index as the body, with offsets
  relative to the actor's feet. Apply no head-anchor offset to the weapon.
- The katana is visible in ready and attack groups 4, 5, 10 and 11. All other
  weapon frames intentionally have empty layers. Unequipped characters should
  have no weapon sheet.
- Equipped swords with view 2, 3 or 49 use the matching katana. Dual wielding is a
  server mechanic; this package has no separate offhand or katar artwork, so
  those extra weapon layers are not drawn.
- The body has 324 indexed images; the katana has one. Formats are SPR 2.1 and
  ACT 2.5. The body packs into a 1024 x 1843 atlas in the current renderer.

## Artwork status

This is the existing first-pass male artwork. Female art, alternate weapon
artwork, independent hair/headgear and class-specific skill effects are not
included. Do not attach unrelated stock weapon animations to the custom hands.

Some attack views use native ACT mirroring, which also reverses costume
asymmetry. Direction consistency, raised-hand grips, blade angles and
foreground/background overlap still need an art polish pass. Matching binary
timelines does not establish perfect visual alignment.

## Provenance and verification

`provenance.json` records the exact original file hashes and generation origin.
The original development package is
`../rathena/generated/custom_jobs/ember_ronin/` relative to the roclient root;
it contains source PNGs, prompts, an interactive preview, the animation manifest,
rebuild tools and the detailed `ART_REVIEW.md` / `CLIENT_INTEGRATION.md` notes.
The checked-in files here are sufficient for runtime use.

`System/samurai_iteminfo.lua` supplies the name, description and weapon view for
item 900000, **Samurai Initiate's Katana**. It references the installed game's
stock Katana item/collection icons; no stock artwork is redistributed in this
overlay. Its separate filename preserves other custom item-info files.

The source package's strict binary validation and the client's independent Rust
parsers accepted both pairs. SHA-256 hashes were checked again when these files
were copied into this overlay. No changes were made to the generated pixels.

The artwork was generated for the user's project from a user-provided character
reference using the built-in image-generation tool. No third-party rights to
that reference are inferred or granted here. No separate artwork license was
supplied with the source package. This provenance note is not a license grant.
