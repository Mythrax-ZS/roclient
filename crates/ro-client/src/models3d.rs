//! 3D monsters: the classes the original client draws from Granny models (data\model\3dmob)
//! instead of sprites (the castle guardians, the Emperium, guild flags, treasure boxes).
//! Their units keep a sprite-less actor for the shadow, name and clicks; the model is posed
//! from the unit's action every frame.

use anyhow::Context;
use ro_formats::gr2::Gr2;
use ro_render::model3d::{Clip, Model3d};

pub struct Loaded {
    model: Model3d,
    /// Per model texture.
    textures: Vec<Option<ro_render::ModelTexId>>,
}

/// Whether a class sprite name is a Granny model.
pub fn is_model(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".gr2")
}

/// Loads a model (`Kguardian90_7.gr2`) and its clips (3dmob_bone\7_move.gr2 ...).
pub fn load(vfs: &ro_vfs::Vfs, r: &mut ro_render::Renderer, file: &str) -> anyhow::Result<Loaded> {
    let file = file.to_ascii_lowercase();
    let path = format!("data\\model\\3dmob\\{file}");
    let g = Gr2::parse(&vfs.read(&path)?.with_context(|| format!("{path} not found"))?)?;
    // The clip set is the number after the last underscore.
    let set = file.trim_end_matches(".gr2").rsplit('_').next().unwrap_or_default().to_string();
    let mut clips = Vec::new();
    for (clip, name) in Clip::FILES {
        let path = format!("data\\model\\3dmob_bone\\{set}_{name}.gr2");
        if let Some(bytes) = vfs.read(&path).ok().flatten() {
            match Gr2::parse(&bytes) {
                Ok(c) => clips.push((clip, c)),
                Err(e) => tracing::warn!("{path}: {e:#}"),
            }
        }
    }
    let textures = g
        .textures
        .iter()
        .map(|t| t.rgba.as_ref().map(|px| r.add_model_texture(&ro_render::Rgba { width: t.width, height: t.height, pixels: px.clone() })))
        .collect();
    tracing::info!(%file, meshes = g.meshes.len(), clips = clips.len(), "3D model loaded");
    Ok(Loaded { model: Model3d::new(&g, &clips), textures })
}

impl Loaded {
    /// The clip for a monster action group (0 idle, 1 walk, 2 attack, 3 hurt, 4 dead).
    fn clip(action: usize) -> Clip {
        match action {
            1 => Clip::Move,
            2 => Clip::Attack,
            3 => Clip::Damage,
            4 => Clip::Dead,
            _ => Clip::Idle,
        }
    }

    /// How long the death clip runs, ms.
    pub fn death_ms(&self) -> f32 {
        self.model.duration(Clip::Dead) * 1000.0
    }

    /// The model drawn for an actor; `dir` is the unit's own facing (0 south, clockwise).
    pub fn draws(&self, a: &ro_render::Actor, dir: usize) -> Vec<ro_render::ModelDraw> {
        let yaw = std::f32::consts::PI + dir as f32 * std::f32::consts::FRAC_PI_4;
        self.model
            .pose(Self::clip(a.action), a.time_ms / 1000.0, a.once, a.pos, yaw, a.scale, a.tint)
            .into_iter()
            .filter_map(|(t, vertices)| Some(ro_render::ModelDraw { texture: (*self.textures.get(t)?)?, vertices }))
            .collect()
    }
}
