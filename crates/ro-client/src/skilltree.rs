//! The skill tree layout from the client's own tables: skilltreeview.lub places each job's
//! skills on a 7-column grid, and jobinheritlist.lub says which job each one came from
//! (Assassin Cross -> Assassin -> Thief -> Novice), giving one tab per tier.

use std::collections::HashMap;

pub const COLUMNS: u32 = 7;

#[derive(Default)]
pub struct SkillTree {
    /// Job id -> (grid slot, internal skill name).
    views: HashMap<i16, Vec<(u32, String)>>,
    /// Transcended 3rd jobs -> the regular 3rd job whose tree they share.
    same_as: HashMap<i16, i16>,
    /// Job id -> the job it advanced from.
    parent: HashMap<i16, i16>,
}

impl SkillTree {
    /// Register a custom job while retaining the stock ancestor tabs.
    pub fn insert(&mut self, job: i16, parent: Option<i16>, slots: Vec<(u32, String)>) {
        self.views.insert(job, slots);
        self.same_as.remove(&job);
        if let Some(parent) = parent {
            self.parent.insert(job, parent);
        } else {
            self.parent.remove(&job);
        }
    }

    pub fn from_lubs(view: &[u8], inherit: &[u8]) -> anyhow::Result<Self> {
        let inherit = ro_formats::lub::eval(inherit)?;
        let mut job_ids: HashMap<String, i16> = HashMap::new();
        let mut parent = HashMap::new();
        let mut same_as = HashMap::new();
        for (name, table) in &inherit.globals {
            for (k, v) in inherit.entries(table) {
                match name.as_str() {
                    "JOBID" => {
                        if let (Some(k), Some(v)) = (k.as_str(), v.as_number()) {
                            job_ids.insert(k.to_string(), v as i16);
                        }
                    }
                    "JOB_INHERIT_LIST2" => {
                        if let (Some(k), Some(v)) = (k.as_number(), v.as_number()) {
                            same_as.insert(k as i16, v as i16);
                        }
                    }
                    "JOB_INHERIT_LIST" => {
                        if let (Some(k), Some(v)) = (k.as_number(), v.as_number()) {
                            parent.insert(k as i16, v as i16);
                        }
                    }
                    _ => {}
                }
            }
        }
        let view = ro_formats::lub::eval(view)?;
        let mut views = HashMap::new();
        for (name, table) in &view.globals {
            if name != "SKILL_TREEVIEW_FOR_JOB" {
                continue;
            }
            for (job, skills) in view.entries(table) {
                let Some(&id) = job.as_str().and_then(|j| job_ids.get(j)) else { continue };
                let slots = view
                    .entries(skills)
                    .iter()
                    .filter_map(|(slot, skill)| Some((slot.as_number()? as u32, skill.as_str()?.to_string())))
                    .collect();
                views.insert(id, slots);
            }
        }
        Ok(Self { views, same_as, parent })
    }

    /// The tabs for a job, first tier first: (job id, its grid).
    pub fn tabs(&self, job: i16) -> Vec<(i16, &[(u32, String)])> {
        let mut out = vec![];
        let mut j = job;
        for _ in 0..8 {
            let view = self.views.get(&j).map(|v| (j, v)).or_else(|| self.same_as.get(&j).and_then(|s| Some((*s, self.views.get(s)?))));
            if let Some((j, v)) = view {
                out.push((j, v.as_slice()));
            }
            match self.parent.get(&j) {
                Some(&p) if p != j => j = p,
                _ => break,
            }
        }
        out.reverse();
        out
    }
}
