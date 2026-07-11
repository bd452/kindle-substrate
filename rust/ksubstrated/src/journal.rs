//! Journal records are intentionally confined to the state tmpfs.  Intent and
//! completion are separate entries so a replacement daemon can distinguish a
//! never-started operation from one that needs an unmount.
use crate::layout::StateTmpfs;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Ord, PartialOrd)]
pub enum Stage { PrepareAlias, BindOriginal, ProtectOriginal, BindWrapper, ProtectWrapper }

impl Stage {
    fn as_str(self) -> &'static str {
        match self {
            Self::PrepareAlias => "prepare-alias",
            Self::BindOriginal => "bind-original",
            Self::ProtectOriginal => "protect-original",
            Self::BindWrapper => "bind-wrapper",
            Self::ProtectWrapper => "protect-wrapper",
        }
    }
    fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "prepare-alias" => Self::PrepareAlias,
            "bind-original" => Self::BindOriginal,
            "protect-original" => Self::ProtectOriginal,
            "bind-wrapper" => Self::BindWrapper,
            "protect-wrapper" => Self::ProtectWrapper,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, Default)]
pub struct RootProgress { pub completed: BTreeSet<Stage>, pub intent: Option<Stage> }

pub struct Journal { path: std::path::PathBuf }

impl Journal {
    pub fn new(state: &StateTmpfs) -> Self { Self { path: state.path().join("mounts.journal") } }
    pub fn intent(&self, stage: Stage, root: &str) -> Result<(), String> { self.write("I", stage, root) }
    pub fn complete(&self, stage: Stage, root: &str) -> Result<(), String> { self.write("C", stage, root) }
    fn write(&self, kind: &str, stage: Stage, root: &str) -> Result<(), String> {
        let mut file = OpenOptions::new().create(true).append(true).open(&self.path)
            .map_err(|e| format!("open journal: {e}"))?;
        writeln!(file, "{kind}\t{}\t{root}", stage.as_str()).map_err(|e| format!("write journal: {e}"))
    }
    pub fn clear(&self) -> Result<(), String> {
        fs::remove_file(&self.path).or_else(|e| if e.kind() == std::io::ErrorKind::NotFound { Ok(()) } else { Err(e) })
            .map_err(|e| format!("clear journal: {e}"))
    }
    pub fn progress(&self) -> Result<BTreeMap<String, RootProgress>, String> {
        let text = match fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
            Err(error) => return Err(format!("read journal: {error}")),
        };
        let mut roots = BTreeMap::<String, RootProgress>::new();
        for line in text.lines() {
            let mut fields = line.splitn(3, '\t');
            let (Some(kind), Some(stage), Some(root)) = (fields.next(), fields.next(), fields.next()) else { return Err("malformed journal entry".to_owned()) };
            let stage = Stage::parse(stage).ok_or_else(|| "unknown journal stage".to_owned())?;
            let progress = roots.entry(root.to_owned()).or_default();
            match kind {
                "I" => progress.intent = Some(stage),
                "C" => { progress.completed.insert(stage); progress.intent = None; }
                _ => return Err("unknown journal entry type".to_owned()),
            }
        }
        Ok(roots)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stage_names_round_trip() { for stage in [Stage::PrepareAlias, Stage::BindOriginal, Stage::ProtectOriginal, Stage::BindWrapper, Stage::ProtectWrapper] { assert_eq!(Stage::parse(stage.as_str()), Some(stage)); } }
}
