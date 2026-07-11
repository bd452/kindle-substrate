//! Persistent registry reads only.  A controller converts validated manifest-v2
//! entries into one immutable session plan; runtime bootstrap never rescans it.
use crate::layout::{MountTmpfs, OriginalAlias, SystemExecutable};
use ksubstrate_targets::{
    self as targets, library_identity, order_manifests, parse_manifest, resolve, Manifest,
    PlanTarget, SessionPlan, TWEAKS_ROOT,
};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

pub struct RegistryEntry {
    pub root: PathBuf,
    pub manifest: Manifest,
}

pub fn build_plan(mounts: &MountTmpfs, generation: u64) -> Result<SessionPlan, String> {
    let platform = targets::platform();
    let entries = discover(Path::new(TWEAKS_ROOT))?;
    let roots = entries
        .iter()
        .map(|entry| (entry.manifest.id.clone(), entry.root.clone()))
        .collect::<std::collections::BTreeMap<_, _>>();
    let manifests = order_manifests(entries.into_iter().map(|entry| entry.manifest).collect())?;
    let mut seen = BTreeSet::new();
    let mut plan_targets = Vec::new();
    for manifest in manifests {
        let root = roots
            .get(&manifest.id)
            .ok_or_else(|| "ordered manifest disappeared".to_owned())?;
        let library = library_identity(
            manifest.id.clone(),
            root.join(&manifest.library),
            manifest.initialization.clone(),
            manifest.dependencies.clone(),
            manifest.order,
        )?;
        for spec in &manifest.targets {
            let target = resolve(spec, platform)?;
            if !seen.insert(target.id.clone()) {
                if let Some(existing) = plan_targets
                    .iter_mut()
                    .find(|candidate: &&mut PlanTarget| candidate.target.id == target.id)
                {
                    existing.libraries.push(library.clone());
                }
                continue;
            }
            let executable = SystemExecutable::from_resolved(&target)?;
            let alias = OriginalAlias::for_system(mounts, &executable)
                .path()
                .to_path_buf();
            plan_targets.push(PlanTarget {
                target,
                alias,
                libraries: vec![library.clone()],
            });
        }
    }
    Ok(SessionPlan {
        generation,
        platform: platform.to_owned(),
        targets: plan_targets,
    })
}

pub fn discover(root: &Path) -> Result<Vec<RegistryEntry>, String> {
    let metadata = match fs::symlink_metadata(root) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("stat tweak registry: {error}")),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("tweak registry is not a real directory".to_owned());
    }
    let mut ids = BTreeSet::new();
    let mut entries = Vec::new();
    for entry in fs::read_dir(root).map_err(|e| format!("read tweak registry: {e}"))? {
        let entry = entry.map_err(|e| format!("read registry entry: {e}"))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        if !targets::valid_id(&name) {
            return Err(format!("invalid registry directory: {name}"));
        }
        let path = entry.path();
        let metadata =
            fs::symlink_metadata(&path).map_err(|e| format!("stat registry entry: {e}"))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(format!("registry entry {name} is not a real directory"));
        }
        let manifest_path = path.join("manifest.json");
        targets::regular_file(&manifest_path)?;
        let manifest = parse_manifest(
            &fs::read_to_string(&manifest_path).map_err(|e| format!("read manifest: {e}"))?,
        )?;
        if !ids.insert(manifest.id.clone()) {
            return Err(format!("duplicate tweak id: {}", manifest.id));
        }
        let library = path.join(&manifest.library);
        targets::regular_file(&library)?;
        if !library.starts_with(root) {
            return Err("manifest library escaped registry".to_owned());
        }
        entries.push(RegistryEntry {
            root: path,
            manifest,
        });
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hidden_entries_are_ignored() {
        let root = std::env::temp_dir().join(format!("ksub-v2-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".staging")).unwrap();
        assert!(discover(&root).unwrap().is_empty());
        let _ = fs::remove_dir_all(root);
    }
}
