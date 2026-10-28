//! Skill effects: which STR animation (data\texture\effect\*.str) plays for a skill, and a
//! cache so each one is parsed and uploaded once.

use std::collections::HashMap;

/// How a skill looks when it lands.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Visual {
    /// An STR animation (data\texture\effect\<name>.str).
    Str(&'static str),
    /// A code-drawn column of light textured with data\texture\effect\<tex>, tinted.
    Column(&'static str, [f32; 3]),
    /// A sprite effect (data\sprite\이팩트\<name>.spr/.act) played once.
    Sprite(&'static str),
}

/// What plays on the target (or cell) when a skill lands, by rAthena skill id.
pub fn skill_visual(skill: u16) -> Option<Visual> {
    Some(match skill {
        28 => Visual::Column("alpha_down.tga", [0.3, 0.55, 0.3]),
        29 => Visual::Column("agi_up.bmp", [0.7, 1.0, 0.8]),
        30 => Visual::Column("alpha_down.tga", [0.25, 0.25, 0.55]),
        34 => Visual::Column("alpha_down.tga", [0.55, 0.5, 0.25]),
        17 => Visual::Sprite("fireball"),
        86 => Visual::Sprite("waterball"),
        // Bolts: the closest STR hits the game ships.
        14 => Visual::Str("icecrash"),
        15 => Visual::Str("freezing"),
        19 => Visual::Str("firehit1"),
        20 | 84 => Visual::Str("lightning"),
        21 => Visual::Str("thunderstorm"),
        83 => Visual::Str("meteor1"),
        _ => Visual::Str(skill_str(skill)?),
    })
}

/// Lasting ground effects (ZC_SKILL_ENTRY unit types), looped until they end.
pub fn ground_str(kind: u32) -> Option<&'static str> {
    Some(match kind {
        0x7e => "safetywall",
        0x7f => "firewall",
        0x83 => "sanctuary",
        0x84 => "magnus",
        0x85 => "pneuma1",
        0x88 => "firepillar",
        0x8e => "quagmire",
        0x8f => "blastmine",
        0x90 => "skidtrap",
        0x92 => "venomdust",
        0x93 => "landmine",
        0x95 => "sandman",
        0x97 => "freezing",
        0x98 => "claymore",
        _ => return None,
    })
}

/// STR for skills whose effect is a plain STR animation.
fn skill_str(skill: u16) -> Option<&'static str> {
    Some(match skill {
        6 => "provoke",
        12 => "safetywall",
        16 => "stonecurse",
        18 => "firewall",
        25 => "pneuma1",
        33 => "angelus",
        35 => "cure",
        45 => "concentration",
        50 => "steal_coin",
        52 => "invenom",
        53 => "detoxication",
        56 => "pierce",
        57 => "brandish",
        58 => "spearstab",
        59 => "spearboomerang",
        60 => "twohand",
        61 => "autocounter",
        62 => "bowling",
        66 => "impositio",
        67 => "suffragium",
        68 => "aspersio",
        69 => "benedictio",
        70 => "sanctuary",
        71 => "slowp",
        72 => "recovery",
        73 => "kyrie",
        74 => "magnificat",
        75 => "gloria",
        76 => "lexdivina",
        78 => "lexaeterna",
        79 => "magnus",
        80 => "firepillar",
        89 => "stormgust",
        92 => "quagmire",
        111 | 459 => "adrenaline",
        112 => "weaponperfection",
        114 => "maximizepower",
        115 => "skidtrap",
        116 => "landmine",
        119 => "sandman",
        121 => "freezing",
        122 => "blastmine",
        123 => "claymore",
        136 => "sonicblow",
        139 => "poisonreact",
        140 => "venomdust",
        141 => "venomsplasher",
        153 => "cartrevolution",
        215 => "strip_weapon",
        216 => "strip_shield",
        217 => "strip_armor",
        218 => "strip_helm",
        250 => "shield_charge",
        255 => "devotion",
        _ => return None,
    })
}

/// Effect for a ZC_NOTIFY_EFFECT id.
pub fn misc_effect(effect: u32) -> Option<&'static str> {
    match effect {
        0 => Some("h_levelup"),
        1 => Some("joblvup"),
        _ => None,
    }
}

#[derive(Default)]
pub struct FxLib {
    cache: HashMap<&'static str, Option<ro_render::FxId>>,
    textures: HashMap<&'static str, Option<ro_render::TexId>>,
    sprites: HashMap<&'static str, Option<ro_render::SheetId>>,
}

impl FxLib {
    /// A texture from data\texture\effect\ for code-drawn effects.
    pub fn texture(&mut self, r: &mut ro_render::Renderer, vfs: &ro_vfs::Vfs, name: &'static str) -> Option<ro_render::TexId> {
        *self.textures.entry(name).or_insert_with(|| {
            let path = format!("data\\texture\\effect\\{name}");
            let img = vfs.read(&path).ok().flatten().and_then(|d| ro_formats::image::decode(&d, &path).ok());
            if img.is_none() {
                tracing::warn!(%path, "effect texture missing");
            }
            Some(r.add_effect_texture(&img?))
        })
    }

    /// A sprite effect from data\sprite\이팩트\.
    pub fn sprite(&mut self, r: &mut ro_render::Renderer, vfs: &ro_vfs::Vfs, name: &'static str) -> Option<ro_render::SheetId> {
        *self.sprites.entry(name).or_insert_with(|| {
            let base = format!("data\\sprite\\이팩트\\{name}");
            let spr = vfs.read(&format!("{base}.spr")).ok().flatten().and_then(|d| ro_formats::spr::Spr::parse(&d).ok());
            let act = vfs.read(&format!("{base}.act")).ok().flatten().and_then(|d| ro_formats::act::Act::parse(&d).ok());
            match (spr, act) {
                (Some(spr), Some(act)) => Some(r.add_sheet(&spr, act, None)),
                _ => {
                    tracing::warn!(%base, "sprite effect missing");
                    None
                }
            }
        })
    }

    /// The uploaded effect, loading it on first use (None if missing or unreadable).
    pub fn get(&mut self, r: &mut ro_render::Renderer, vfs: &ro_vfs::Vfs, name: &'static str) -> Option<ro_render::FxId> {
        *self.cache.entry(name).or_insert_with(|| {
            let path = format!("data\\texture\\effect\\{name}.str");
            let str = vfs
                .read(&path)
                .ok()
                .flatten()
                .and_then(|d| ro_formats::strm::Str::parse(&d).inspect_err(|e| tracing::warn!(%path, "{e:#}")).ok())?;
            let images = str
                .layers
                .iter()
                .map(|l| {
                    l.textures
                        .iter()
                        .map(|t| {
                            let p = format!("data\\texture\\effect\\{t}");
                            vfs.read(&p).ok().flatten().and_then(|d| ro_formats::image::decode(&d, &p).ok())
                        })
                        .collect()
                })
                .collect();
            tracing::debug!(%path, layers = str.layers.len(), "loaded effect");
            Some(r.add_fx(str, images))
        })
    }
}
