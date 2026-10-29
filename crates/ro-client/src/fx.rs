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

/// How an RSW ambient effect (its id is an EF_* id) looks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MapVisual {
    /// One of these STRs, chosen at random, fired once per emit.
    Str(&'static [&'static str]),
    /// A sprite effect looping in place for as long as the map is loaded.
    Sprite(&'static str),
    /// Code-drawn drifting motes (fireflies).
    Motes,
}

pub fn map_effect(id: i32) -> Option<MapVisual> {
    Some(match id {
        45 => MapVisual::Motes,
        47 => MapVisual::Sprite("torch_01"),
        109 => MapVisual::Str(&["bubble1", "bubble2", "bubble3", "bubble4"]),
        _ => return None,
    })
}

/// Emitters re-fire only while the player is this close (cells), as the original client.
pub const MAP_FX_RANGE: f32 = 25.0;

/// An RSW effect emitter on the current map.
pub struct MapEmitter {
    pub visual: Option<MapVisual>,
    pub pos: glam::Vec3,
    /// Milliseconds between emits (the RSW value × 10).
    pub every_ms: f32,
    /// Countdown to the next emit; sprites use it as their animation clock.
    pub clock_ms: f32,
    /// Emits so far (varies which STR plays).
    pub fired: u32,
}

impl MapEmitter {
    /// `index` staggers emitters so a field of them doesn't fire in step.
    pub fn new(id: i32, pos: glam::Vec3, delay: f32, index: usize) -> Self {
        let every_ms = (delay * 10.0).max(200.0);
        Self { visual: map_effect(id), pos, every_ms, clock_ms: (index as f32 * 397.0) % every_ms, fired: index as u32 }
    }
}

/// The map's particle emitters: RSW effect objects with id 974 (EF_EMITTER, `positions` in
/// RSW order) paired with the entries of `effecttool\<map>.lub` (the Nth object takes entry
/// N). Missing fields take BrowEdit's defaults. `animated`: the id 1073 objects instead
/// (EF_ANIMATED_EMITTER, `_<map>_animatedEmitterInfo`), whose particles play built-in frame
/// sets rather than one texture.
pub fn map_emitters(fx: &mut FxLib, r: &mut ro_render::Renderer, vfs: &ro_vfs::Vfs, map: &str, positions: &[glam::Vec3], animated: bool) -> Vec<ro_render::Emitter> {
    if positions.is_empty() {
        return vec![];
    }
    let data = [format!("data\\luafiles514\\lua files\\effecttool\\{map}.lub"), format!("data\\lua files\\effecttool\\{map}.lub")]
        .iter()
        .find_map(|p| vfs.read(p).ok().flatten());
    let Some(data) = data else { return vec![] };
    let chunk = match ro_formats::lub::eval(&data) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(map, "effecttool lub: {e:#}");
            return vec![];
        }
    };
    let kind = if animated { "animatedEmitterInfo" } else { "emitterInfo" };
    let Some(table) = chunk.globals.get(&format!("_{}_{kind}", map.replace('@', ""))) else { return vec![] };
    let frames: Vec<Vec<ro_render::TexId>> = if animated {
        [&["shockwave_b", "shockwave_c", "shockwave_d", "shockwave_e"][..], &["plazma_a", "plazma_b", "plazma_c"][..]]
            .iter()
            .map(|set| set.iter().filter_map(|n| fx.texture_dyn(r, vfs, &format!("effect\\{n}.bmp"))).collect())
            .collect()
    } else {
        vec![]
    };
    let num = |v: &ro_formats::lub::Value| if let ro_formats::lub::Value::Number(n) = v { Some(*n as f32) } else { None };
    let mut out = vec![];
    for (i, &pos) in positions.iter().enumerate() {
        let Some(e) = chunk.field_index(table, i as u32) else { continue };
        let list = |key: &str| -> Vec<f32> { chunk.field(e, key).map(|t| chunk.entries(t).iter().filter_map(|(_, v)| num(v)).collect()).unwrap_or_default() };
        let v3 = |key: &str, d: [f32; 3]| {
            let l = list(key);
            if l.len() >= 3 { [l[0], l[1], l[2]] } else { d }
        };
        let v2 = |key: &str, d: [f32; 2]| {
            let l = list(key);
            match l.len() {
                0 => d,
                1 => [l[0], l[0]],
                _ => [l[0], l[1]],
            }
        };
        let one = |key: &str, d: f32| list(key).first().copied().unwrap_or(d);
        let color = list("color");
        let color = if color.len() >= 4 { [color[0] / 255.0, color[1] / 255.0, color[2] / 255.0, color[3] / 255.0] } else { [1.0; 4] };
        let texture = match chunk.field(e, "texture") {
            Some(ro_formats::lub::Value::Str(s)) => s.replace("\\\\", "\\"),
            _ => "smoke2.bmp".to_string(),
        };
        let Some(tex) = fx.texture_dyn(r, vfs, &texture) else { continue };
        out.push(ro_render::Emitter {
            pos,
            def: ro_render::EmitterDef {
                dir1: v3("dir1", [0.0; 3]),
                dir2: v3("dir2", [0.0; 3]),
                gravity: v3("gravity", [0.0; 3]),
                radius: v3("radius", [0.0; 3]),
                color,
                rate: v2("rate", [5.0, 15.0]),
                size: v2("size", [3.0, 8.0]),
                life: v2("life", [1.0, 5.0]),
                speed: one("speed", 0.5),
                src: one("srcmode", 10.0) as u8,
                dst: one("destmode", 2.0) as u8,
                max: one("maxcount", 30.0).clamp(0.0, 500.0) as u32,
                zenable: one("zenable", 1.0) != 0.0,
                linear: false,
                billboard: one("billboard_off", 0.0) == 0.0,
                rotate: v3("rotate_angle", [0.0; 3]),
                tex,
                frames: frames.clone(),
            },
        });
    }
    out
}

/// Coloured map torches (EF_TORCH_RED 690, EF_TORCH_GREEN 691, EF_TORCH_PURPLE 696): the
/// original draws them in code as a camera-facing flame cycling `effect\torch_<colour>01..13`
/// (black-keyed, added). Each becomes one never-ending particle on the shared particle system,
/// its flame's foot on the RSW position like the plain torch sprite.
pub fn map_torches(fx: &mut FxLib, r: &mut ro_render::Renderer, vfs: &ro_vfs::Vfs, effects: &[(i32, glam::Vec3)]) -> Vec<ro_render::Emitter> {
    const HALF: f32 = 4.0;
    let mut sets: HashMap<i32, Option<Vec<ro_render::TexId>>> = HashMap::new();
    let mut out = vec![];
    for &(id, pos) in effects {
        let colour = match id {
            690 => "red",
            691 => "green",
            696 => "violet",
            _ => continue,
        };
        let frames = sets.entry(id).or_insert_with(|| {
            let f: Vec<_> = (1..=13).filter_map(|n| fx.texture_dyn(r, vfs, &format!("effect\\torch_{colour}{n:02}.bmp"))).collect();
            (!f.is_empty()).then_some(f)
        });
        let Some(frames) = frames.clone() else { continue };
        out.push(ro_render::Emitter {
            // The particle system lifts emitters 10 units; set the flame's foot on the spot.
            pos: pos + glam::Vec3::new(0.0, HALF - 10.0, 0.0),
            def: ro_render::EmitterDef {
                dir1: [0.0; 3],
                dir2: [0.0; 3],
                gravity: [0.0; 3],
                radius: [0.0; 3],
                color: [1.0; 4],
                rate: [0.001, 0.001],
                size: [HALF, HALF],
                life: [1.0e6, 1.0e6],
                speed: 0.0,
                src: 2,
                dst: 2,
                max: 1,
                zenable: true,
                linear: true,
                billboard: true,
                rotate: [0.0; 3],
                tex: frames[0],
                frames: vec![frames],
            },
        });
    }
    out
}

/// The map's wind effects: RSW objects with id 2343 (EF_WIND_EFFECT, `positions` in RSW
/// order) paired with `_<map>_windEffectInfo` entries the same way as emitters.
pub fn map_winds(fx: &mut FxLib, r: &mut ro_render::Renderer, vfs: &ro_vfs::Vfs, map: &str, positions: &[glam::Vec3]) -> Vec<ro_render::Wind> {
    if positions.is_empty() {
        return vec![];
    }
    let Some(data) = [format!("data\\luafiles514\\lua files\\effecttool\\{map}.lub"), format!("data\\lua files\\effecttool\\{map}.lub")].iter().find_map(|p| vfs.read(p).ok().flatten()) else {
        return vec![];
    };
    let Ok(chunk) = ro_formats::lub::eval(&data) else { return vec![] };
    let Some(table) = chunk.globals.get(&format!("_{}_windEffectInfo", map.replace('@', ""))) else { return vec![] };
    use ro_formats::lub::Value;
    let num = |v: Option<&Value>| if let Some(Value::Number(n)) = v { Some(*n as f32) } else { None };
    let mut out = vec![];
    for (i, &pos) in positions.iter().enumerate() {
        let Some(e) = chunk.field_index(table, i as u32) else { continue };
        let list = |key: &str| -> Vec<f32> { chunk.field(e, key).map(|t| chunk.entries(t).iter().filter_map(|(_, v)| num(Some(v))).collect()).unwrap_or_default() };
        let one = |key: &str, d: f32| num(chunk.field(e, key)).unwrap_or(d);
        let texture = match chunk.field(e, "texture") {
            Some(Value::Str(s)) => s.replace("\\\\", "\\"),
            _ => continue,
        };
        let Some(tex) = fx.texture_dyn(r, vfs, &texture) else { continue };
        // Stored ARGB (BrowEdit: "the lub color format is argb").
        let c = list("color");
        let color = if c.len() >= 4 { [c[1] / 255.0, c[2] / 255.0, c[3] / 255.0, c[0] / 255.0] } else { [1.0; 4] };
        let rot = list("rotateVector");
        let count = one("particleNum", 0.0).max(0.0) as u32;
        let seeds = (0..count).map(|k| {
            let h = |g: f32| ((k as f32 * 12.9898 + g * 78.233 + i as f32 * 3.7).sin() * 43758.547).fract().abs();
            (h(1.0), h(2.0))
        }).collect();
        out.push(ro_render::Wind {
            pos,
            count,
            color,
            radius: one("radius", 5.0),
            thickness: one("thickness", 1.0),
            height: one("height", 0.0),
            speed: one("speed", 1.0),
            full_angle: one("fullAngle", 360.0),
            rotate: [rot.first().copied().unwrap_or(0.0), rot.get(1).copied().unwrap_or(0.0)],
            src: one("srcMode", 5.0) as u8,
            dst: one("destMode", 2.0) as u8,
            tex,
            seeds,
        });
    }
    out
}

#[derive(Default)]
pub struct FxLib {
    /// Textures by path under data\texture\ (particle emitters name them at run time).
    textures_dyn: HashMap<String, Option<ro_render::TexId>>,
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

    /// A texture by path under data\texture\ (or data\texture\effect\ when not found there).
    pub fn texture_dyn(&mut self, r: &mut ro_render::Renderer, vfs: &ro_vfs::Vfs, name: &str) -> Option<ro_render::TexId> {
        if let Some(t) = self.textures_dyn.get(&name.to_ascii_lowercase()) {
            return *t;
        }
        let img = [format!("data\\texture\\{name}"), format!("data\\texture\\effect\\{name}")]
            .into_iter()
            .find_map(|p| vfs.read(&p).ok().flatten().and_then(|d| ro_formats::image::decode(&d, &p).ok()));
        if img.is_none() {
            tracing::warn!(%name, "particle texture missing");
        }
        let t = img.map(|i| r.add_effect_texture(&i));
        self.textures_dyn.insert(name.to_ascii_lowercase(), t);
        t
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
