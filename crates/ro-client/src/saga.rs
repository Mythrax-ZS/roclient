//! The Twin Flames Saga's monsters (rAthena db/custom/samurai/mob_db.yml, ids 31900..31939).
//! The server sends their own class ids; this client draws each one from a stock sprite,
//! recoloured, resized and tinted, with a code-drawn aura for the gods. The original client
//! has no sprite for these ids.

use ro_render::AuraStyle;

/// How a saga monster looks.
#[derive(Clone, Copy, Debug)]
pub struct Look {
    /// Stock sprite (data\sprite\몬스터\ name) it is drawn from.
    pub base: &'static str,
    /// Palette shift: hue turned by degrees, saturation and lightness scaled.
    pub hue: f32,
    pub saturation: f32,
    pub lightness: f32,
    /// Colour multiplier over the whole sprite (also reaches its true-colour frames).
    pub tint: [f32; 3],
    pub scale: f32,
    pub aura: Option<AuraStyle>,
}

const PLAIN: Look = Look { base: "", hue: 0.0, saturation: 1.0, lightness: 1.0, tint: [1.0; 3], scale: 1.0, aura: None };

/// The look of saga monster `class`, if it is one.
pub fn look(class: i16) -> Option<Look> {
    LOOKS.iter().find(|(id, _)| *id == class).map(|(_, l)| *l)
}

/// Saga monsters by class id. Kept in step with db/custom/samurai/mob_db.yml.
const LOOKS: &[(i16, Look)] = &[
    // Ashigaru Raider: a wanderer in dirty, faded cloth.
    (31900, Look { base: "wander_man", saturation: 0.6, lightness: 0.75, tint: [0.9, 0.85, 0.8], ..PLAIN }),
    // Crimson Fields Soldier: a spearman stained red.
    (31901, Look { base: "hyegun", hue: -35.0, saturation: 1.4, lightness: 0.9, tint: [1.0, 0.75, 0.75], ..PLAIN }),
    // Cultist of Ashes: ash grey, lit from within by embers.
    (31902, Look { base: "wander_man", saturation: 0.3, lightness: 0.45, tint: [1.0, 0.7, 0.6], ..PLAIN }),
    // Warlord Kurogane: black iron with a bronze sheen.
    (31903, Look { base: "incantation_samurai", lightness: 0.85, tint: [1.0, 0.92, 0.75], scale: 1.1, ..PLAIN }),
    // Shade of Yami: a reaper of ash, smouldering.
    (31904, Look { base: "grim_reaper_ankou", saturation: 0.5, lightness: 0.35, tint: [1.0, 0.55, 0.4], scale: 0.9, aura: Some(AuraStyle::Yami), ..PLAIN }),
    // Usurper General Mibu: the Frozen Citadel's frost on steel.
    (31905, Look { base: "lord_of_death", lightness: 0.9, tint: [0.75, 0.85, 1.0], scale: 0.85, ..PLAIN }),
    // Fallen Monk Genkai: a darkened, bloodied priest.
    (31906, Look { base: "fallingbishop", hue: 30.0, lightness: 0.7, tint: [0.85, 0.8, 0.8], ..PLAIN }),
    // Kage, Assassin of Ashes: a shinobi of soot and ember.
    (31907, Look { base: "shinobi", saturation: 0.4, lightness: 0.55, tint: [1.0, 0.6, 0.5], scale: 1.1, ..PLAIN }),
    // Guardian of the Seal: a great golden Sarugami mask-spirit.
    (31908, Look { base: "tengu", hue: 28.0, lightness: 1.05, tint: [1.0, 0.92, 0.7], scale: 1.35, ..PLAIN }),
    // High Priestess Sayo: the goddess's servant, robed in pale light.
    (31909, Look { base: "miyabi_ningyo", saturation: 0.5, lightness: 1.4, tint: [1.0, 0.95, 0.8], scale: 1.15, aura: Some(AuraStyle::Amaterasu), ..PLAIN }),
    // Glacial Yokai: glacier teal.
    (31910, Look { base: "ktullanux", hue: 20.0, lightness: 1.1, ..PLAIN }),
    // Daimyo Kurozane: lacquered violet armour of the Palace of Masks.
    (31911, Look { base: "incantation_samurai", hue: 270.0, saturation: 1.6, lightness: 0.9, tint: [0.85, 0.7, 1.0], scale: 1.1, ..PLAIN }),
    // Herald of Ashes: a dark lord smouldering red.
    (31912, Look { base: "dark_lord", tint: [1.0, 0.6, 0.45], scale: 0.9, aura: Some(AuraStyle::Yami), ..PLAIN }),
    // Contract Binder: a gilded watcher of the goddess.
    (31913, Look { base: "observation", lightness: 1.1, tint: [1.0, 0.95, 0.75], aura: Some(AuraStyle::Amaterasu), ..PLAIN }),
    // Kagutsuchi-no-Yami, God of Ashes and Chains: a fire spirit burned down to black, embers within.
    (31914, Look { base: "ifrit", saturation: 0.8, lightness: 0.5, tint: [1.0, 0.7, 0.55], scale: 1.3, aura: Some(AuraStyle::Yami), ..PLAIN }),
    // Amaterasu-no-Mikazuki, radiant: white and sun gold.
    (31915, Look { base: "randgris", saturation: 0.6, lightness: 1.3, tint: [1.0, 0.92, 0.7], scale: 1.2, aura: Some(AuraStyle::Amaterasu), ..PLAIN }),
    // Eclipsed: violet and shadow.
    (31916, Look { base: "randgris", hue: 260.0, lightness: 0.45, tint: [0.75, 0.6, 1.0], scale: 1.2, aura: Some(AuraStyle::Yami), ..PLAIN }),
    // The Weaver unmasked: light and shadow fused, crimson gold.
    (31917, Look { base: "randgris", lightness: 0.9, tint: [1.0, 0.75, 0.6], scale: 1.35, aura: Some(AuraStyle::Amaterasu), ..PLAIN }),
];

impl Look {
    /// `palette` recoloured by this look (index 0, the colour key, is left alone).
    pub fn recolour(&self, palette: &[[u8; 4]; 256]) -> [[u8; 4]; 256] {
        let mut out = *palette;
        for c in out.iter_mut().skip(1) {
            let (h, s, l) = hsl([c[0], c[1], c[2]]);
            let rgb = rgb(((h + self.hue) % 360.0 + 360.0) % 360.0, (s * self.saturation).clamp(0.0, 1.0), (l * self.lightness).clamp(0.0, 1.0));
            c[..3].copy_from_slice(&rgb);
        }
        out
    }

    pub fn recoloured(&self) -> bool {
        self.hue != 0.0 || self.saturation != 1.0 || self.lightness != 1.0
    }
}

/// RGB (0..255) to hue (degrees), saturation and lightness (0..1).
fn hsl(c: [u8; 3]) -> (f32, f32, f32) {
    let [r, g, b] = c.map(|v| v as f32 / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = (max + min) / 2.0;
    if max == min {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
    let h = if max == r {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    (h * 60.0, s, l)
}

fn rgb(h: f32, s: f32, l: f32) -> [u8; 3] {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    [r, g, b].map(|v| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colour_round_trip() {
        for c in [[200, 30, 40], [10, 200, 90], [255, 255, 255], [0, 0, 0], [90, 90, 250]] {
            let (h, s, l) = hsl(c);
            let back = rgb(h, s, l);
            for i in 0..3 {
                assert!((back[i] as i32 - c[i] as i32).abs() <= 1, "{c:?} -> {back:?}");
            }
        }
    }

    #[test]
    fn hue_turns_red_to_blue() {
        let look = Look { hue: 240.0, ..PLAIN };
        let mut pal = [[0u8; 4]; 256];
        pal[1] = [255, 0, 0, 0];
        assert_eq!(look.recolour(&pal)[1][..3], [0, 0, 255]);
        // The colour key stays.
        assert_eq!(look.recolour(&pal)[0], [0, 0, 0, 0]);
    }

    #[test]
    fn ids_are_in_the_saga_range() {
        assert!(LOOKS.iter().all(|(id, l)| (31900..31940).contains(id) && !l.base.is_empty()));
    }
}
