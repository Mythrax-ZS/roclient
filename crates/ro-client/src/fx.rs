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
    /// Drawn by the client's own code where the hit is handled (Grimtooth, Soul Breaker...).
    Code,
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
        379 | 406 => Visual::Code,
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
    cache: HashMap<String, Option<ro_render::FxId>>,
    textures: HashMap<&'static str, Option<ro_render::TexId>>,
    sprites: HashMap<&'static str, Option<ro_render::SheetId>>,
    /// STR paths under effect that sit in subfolders (see `index`).
    strs: Vec<String>,
}

impl FxLib {
    /// Indexes the STR effects kept in per-skill folders (effect\savage_impact\...\x.str),
    /// so skills without a hand-mapped effect can find their own by name.
    pub fn index(&mut self, vfs: &ro_vfs::Vfs) {
        const ROOT: &str = r"data\texture\effect\";
        self.strs = vfs
            .names()
            .filter_map(|n| n.to_ascii_lowercase().strip_prefix(ROOT)?.strip_suffix(".str").map(str::to_string))
            .filter(|n| n.contains('\\') && !n.starts_with(r"mineffect\"))
            .collect();
        self.strs.sort();
        tracing::info!(count = self.strs.len(), "indexed skill effect folders");
    }

    /// The best STR (path under effect\, no extension) for a skill's short name, e.g.
    /// "savage_impact" -> savage_impact\savage_impact\savage_impact. Cast, ground and
    /// counter pieces are skipped in favour of the main or hit animation.
    pub fn find(&self, short: &str) -> Option<&str> {
        let joined = short.replace('_', "");
        let keys = [short.to_string(), joined.clone(), format!("new_{short}"), format!("new_{joined}")];
        let score = |p: &str| -> Option<u32> {
            let parts: Vec<&str> = p.split('\\').collect();
            let stem = parts.last()?.trim_start_matches("min_");
            let key = keys.iter().find(|k| parts.iter().any(|c| c == k))?;
            let mut s = 100;
            if stem == key {
                s = 0;
            } else if stem.ends_with("_hit") || stem.ends_with("_hit_01") {
                s = 10;
            }
            if ["cast", "bottom", "count", "clock", "ground", "_end", "_loop"].iter().any(|w| stem.contains(w)) {
                s += 50;
            }
            Some(s + parts.len() as u32)
        };
        self.strs.iter().filter_map(|p| Some((score(p)?, p.as_str()))).min().map(|(_, p)| p)
    }

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
    pub fn get(&mut self, r: &mut ro_render::Renderer, vfs: &ro_vfs::Vfs, name: &str) -> Option<ro_render::FxId> {
        *self.cache.entry(name.to_string()).or_insert_with(|| {
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
                            // Effects in subfolders name textures beside themselves.
                            let dir = name.rsplit_once('\\').map_or("", |(d, _)| d);
                            [format!("data\\texture\\effect\\{dir}\\{t}"), format!("data\\texture\\effect\\{t}")]
                                .into_iter()
                                .find_map(|p| vfs.read(&p).ok().flatten().and_then(|d| ro_formats::image::decode(&d, &p).ok()))
                        })
                        .collect()
                })
                .collect();
            tracing::debug!(%path, layers = str.layers.len(), "loaded effect");
            Some(r.add_fx(str, images))
        })
    }
}

/// rAthena `EF_*` effects the original draws in code, shown with the matching skill's effect.
pub fn ef_skill(effect: u32) -> Option<u16> {
    Some(match effect {
        312 | 313 => 28,   // HEAL, HEAL2: Heal
        226 => 254,        // GRANDCROSS
        18 => 50,          // STEAL
        16 => 5,           // BASH
        53 => 52,          // POISONHIT: Envenom
        125 => 138,        // ENCHANTPOISON
        215 => 196,        // SUMMONSLAVE
        106 => 122,        // BLASTMINEBOMB
        72 => 261,         // SPHERE: Call Spirits
        99 => 120,         // FLASHER
        496 => 249,        // GUARD2: Auto Guard
        9 | 60 => 45,      // ENHANCE, LOCKON: Improve Concentration
        71 => 29,          // CONE: Increase AGI
        _ => return None,
    })
}

pub fn script_effect(effect: u32) -> Option<&'static str> {
    ef_str(effect)
}

/// rAthena `EF_*` effect ids whose name matches an STR in the client data (generated from
/// src/map/script.hpp against data\texture\effect).
fn ef_str(effect: u32) -> Option<&'static str> {
    Some(match effect {
        21 => "detoxication",
        23 => "stonecurse",
        25 => "firewall",
        30 => "thunderstorm",
        41 => "angelus",
        49 => "firehit",
        52 => "windhit",
        64 => "arrowshot",
        65 => "invenom",
        66 => "cure",
        67 => "provoke",
        68 => "mvp",
        69 => "skidtrap",
        75 => "gloria",
        76 => "magnificat",
        77 => "resurrection",
        78 => "recovery",
        81 => "pierce",
        83 => "sanctuary",
        84 => "impositio",
        85 => "lexaeterna",
        86 => "aspersio",
        87 => "lexdivina",
        88 => "suffragium",
        89 => "stormgust",
        90 => "lord",
        91 => "benedictio",
        95 => "quagmire",
        96 => "firepillar",
        97 => "firepillarbomb",
        101 => "repairweapon",
        102 => "crashearth",
        105 => "blastmine",
        107 => "claymore",
        108 => "freezing",
        109 => "bubble",
        110 => "gaspush",
        112 => "kyrie",
        113 => "magnus",
        121 => "sonicblow",
        124 => "venomdust",
        126 => "poisonreact",
        131 => "autocounter",
        133 => "freeze",
        134 => "freezed",
        135 => "icecrash",
        139 => "sandman",
        144 => "brandish2",
        145 => "shockwave",
        146 => "shockwavehit",
        147 => "earthhit",
        152 => "holyhit",
        153 => "concentration",
        158 => "joblvup",
        163 => "sakura",
        169 => "energycoat",
        170 => "cartrevolution",
        181 => "mentalbreak",
        183 => "sui_explosion",
        185 => "suicide",
        199 => "pong",
        213 => "deffender",
        214 => "keeping",
        248 => "providence",
        251 => "devotion",
        311 => "loud",
        335 => "poison",
        371 => "angel",
        372 => "devil",
        507 => "mapae",
        508 => "itempokjuk",
        603 => "firehit2",
        689 => "dust",
        708 => "storm_min",
        813 => "enervation",
        920 => "firewall2",
        959 => "poison_mist",
        960 => "eraser_cutter",
        964 => "lava_slide",
        965 => "sonic_claw",
        998 => "red_cross",
        1031 => "invincibleoff2",
        1034 => "moonstar",
        1035 => "strangelights",
        1040 => "gc_darkcrow",
        1042 => "all_full_throttle",
        1043 => "sr_flashcombo",
        1044 => "rk_luxanima",
        1046 => "so_elemental_shield",
        1047 => "ab_offertorium",
        1048 => "wl_telekinesis_intense",
        1049 => "gn_illusiondoping",
        1050 => "nc_magma_eruption",
        1057 => "ab_offertorium_ring",
        1064 => "fire_rain",
        1078 => "s_storm",
        1084 => "fstone",
        1085 => "qscaraba",
        1086 => "ljosalfar",
        1089 => "maple_falls",
        1091 => "magical_feather",
        1093 => "gift_of_snow",
        1095 => "time_accessory",
        1123 => "su_grooming",
        1124 => "su_chattering",
        1133 => "firedance",
        1219 => "sj_document",
        1220 => "falling_star",
        1229 => "newmoon_kick",
        1230 => "fullmoon_kick",
        _ => return None,
    })
}

/// Hat effects (ZC_EQUIPMENT_EFFECT ids) -> STR under data\texture\effect, from
/// hateffectinfo\hateffectids.lub (id per name) and hateffectinfo.lub (file per name).
pub fn hat_effects(vfs: &ro_vfs::Vfs) -> HashMap<u16, &'static str> {
    const DIR: &str = r"data\luafiles514\lua files\hateffectinfo";
    let load = |f: &str| vfs.read(&format!(r"{DIR}\{f}.lub")).ok().flatten().and_then(|d| ro_formats::lub::eval(&d).ok());
    let (Some(ids), Some(info)) = (load("hateffectids"), load("hateffectinfo")) else { return HashMap::new() };
    let (Some(id_tbl), Some(info_tbl)) = (ids.globals.get("HatEFID"), info.globals.get("hatEffectTable")) else { return HashMap::new() };
    let mut out = HashMap::new();
    for (name, id) in ids.entries(id_tbl) {
        let (Some(name), Some(id)) = (name.as_str(), id.as_number()) else { continue };
        let file = info.entries(info_tbl).iter().find(|(k, _)| k.as_str() == Some(name)).and_then(|(_, t)| info.field(t, "resourceFileName")).and_then(|v| v.as_str());
        if let Some(file) = file {
            let short: &'static str = Box::leak(file.trim_end_matches(".str").trim_end_matches(".STR").to_string().into_boxed_str());
            out.insert(id as u16, short);
        }
    }
    tracing::info!(count = out.len(), "hat effects");
    out
}
