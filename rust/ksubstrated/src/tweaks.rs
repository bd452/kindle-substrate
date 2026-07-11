//! Read-only tweak registry.  Hidden staging/retired directories are never a
//! registry view, and every visible package is validated before it can affect
//! wrapper roots.
use crate::layout::{is_blacklisted, TWEAKS_ROOT};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Tweak { pub id: String, pub path: PathBuf, pub filter: PathBuf, pub library: PathBuf }

pub fn roots() -> Result<Vec<String>, String> {
    let registry = discover(Path::new(TWEAKS_ROOT), current_platform())?;
    let mut roots = vec!["/usr/bin/appmgrd".to_owned(), "/usr/bin/pillow".to_owned(), "/usr/sbin/pillow".to_owned()];
    for tweak in registry {
        let filter = fs::read_to_string(&tweak.filter).map_err(|e| format!("read filter for {}: {e}", tweak.id))?;
        for name in filter_names(&filter) {
            if is_blacklisted(&name) { continue; }
            for directory in ["/usr/bin", "/usr/sbin", "/bin", "/sbin"] {
                let candidate = format!("{directory}/{name}");
                if Path::new(&candidate).exists() && !roots.contains(&candidate) { roots.push(candidate); }
            }
        }
    }
    Ok(roots)
}

pub fn discover(root: &Path, platform: &str) -> Result<Vec<Tweak>, String> {
    let root_metadata = match fs::symlink_metadata(root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("stat tweak registry: {error}")),
    };
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() { return Err("tweak registry is not a real directory".to_owned()); }
    let mut ids = BTreeSet::new();
    let mut tweaks = Vec::new();
    for entry in fs::read_dir(root).map_err(|e| format!("read tweak registry: {e}"))? {
        let entry = entry.map_err(|e| format!("read tweak registry entry: {e}"))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') { continue; }
        if !valid_id(&name) { return Err(format!("invalid visible tweak directory name: {name}")); }
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|e| format!("stat tweak {name}: {e}"))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() { return Err(format!("tweak {name} is not a real directory")); }
        let manifest = read_regular(&path.join("manifest.json"), "manifest")?;
        let id = json_string(&manifest, "id").ok_or_else(|| format!("tweak {name} manifest has no id"))?;
        if !valid_id(&id) || !ids.insert(id.clone()) { return Err(format!("invalid or duplicate tweak id: {id}")); }
        if let Some(platforms) = json_string_array(&manifest, "supported_platforms") {
            if !platforms.iter().any(|value| value == platform) { return Err(format!("tweak {id} does not support {platform}")); }
        }
        let filter_name = json_string(&manifest, "filter").unwrap_or_else(|| "tweak.ksfilter".to_owned());
        let library_name = json_string(&manifest, "library").unwrap_or_else(|| "tweak.so".to_owned());
        if !simple_file_name(&filter_name) || !simple_file_name(&library_name) { return Err(format!("tweak {id} has unsafe manifest file paths")); }
        let filter = path.join(filter_name);
        let library = path.join(library_name);
        read_regular(&filter, "filter")?;
        regular_file(&library, "library")?;
        tweaks.push(Tweak { id, path, filter, library });
    }
    validate_relationships(&tweaks, root)?;
    Ok(tweaks)
}

fn validate_relationships(tweaks: &[Tweak], root: &Path) -> Result<(), String> {
    let ids: BTreeSet<_> = tweaks.iter().map(|tweak| tweak.id.as_str()).collect();
    for tweak in tweaks {
        let manifest = read_regular(&tweak.path.join("manifest.json"), "manifest")?;
        for dependency in relationship_ids(&manifest, "dependencies") {
            if dependency != "com.bd452.ksubstrate" && !ids.contains(dependency.as_str()) { return Err(format!("tweak {} is missing dependency {dependency}", tweak.id)); }
        }
        for conflict in relationship_ids(&manifest, "conflicts") {
            if ids.contains(conflict.as_str()) { return Err(format!("tweak {} conflicts with {conflict}", tweak.id)); }
        }
        if !tweak.path.starts_with(root) || !tweak.filter.starts_with(root) || !tweak.library.starts_with(root) { return Err(format!("tweak {} escaped registry", tweak.id)); }
    }
    Ok(())
}

fn read_regular(path: &Path, label: &str) -> Result<String, String> {
    regular_file(path, label)?;
    fs::read_to_string(path).map_err(|e| format!("read {label} {}: {e}", path.display()))
}
fn regular_file(path: &Path, label: &str) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|e| format!("missing {label} {}: {e}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() { return Err(format!("{label} is not a regular file: {}", path.display())); }
    Ok(())
}

fn filter_names(filter: &str) -> impl Iterator<Item = String> + '_ {
    filter.lines().filter_map(|line| {
        let token = line.split('#').next().unwrap_or("").trim();
        (!token.is_empty() && token != "*").then(|| token.to_owned())
    })
}

fn current_platform() -> &'static str { if Path::new("/lib/ld-linux-armhf.so.3").exists() { "kindlehf" } else { "kindlepw2" } }
fn valid_id(value: &str) -> bool { !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')) }
fn simple_file_name(value: &str) -> bool { !value.is_empty() && Path::new(value).components().count() == 1 && value != "." && value != ".." }

// The manifest format is deliberately small and generated by KPM tooling. This
// parser accepts only unescaped string values/arrays; malformed JSON therefore
// fails closed instead of allowing a path or registry escape.
fn json_string(input: &str, key: &str) -> Option<String> {
    let marker = format!("\"{key}\"");
    let (_, rest) = input.split_once(&marker)?;
    let rest = rest.trim_start().strip_prefix(':')?.trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    let value = &rest[..end];
    (!value.contains('\\')).then(|| value.to_owned())
}

fn json_string_array(input: &str, key: &str) -> Option<Vec<String>> {
    let marker = format!("\"{key}\"");
    let (_, rest) = input.split_once(&marker)?;
    let rest = rest.trim_start().strip_prefix(':')?.trim_start().strip_prefix('[')?;
    let end = rest.find(']')?;
    let values = rest[..end].split(',').map(str::trim).filter(|value| !value.is_empty()).map(|value| value.strip_prefix('"')?.strip_suffix('"').filter(|value| !value.contains('\\')).map(str::to_owned)).collect::<Option<Vec<_>>>()?;
    Some(values)
}

fn relationship_ids(input: &str, key: &str) -> Vec<String> {
    let Some(body) = json_array_body(input, key) else { return Vec::new(); };
    if !body.contains('{') {
        return body.split(',').map(str::trim).filter_map(|value| value.strip_prefix('"')?.strip_suffix('"').filter(|value| !value.contains('\\')).map(str::to_owned)).collect();
    }
    let mut ids = Vec::new();
    let mut remaining = body;
    while let Some((_, after_id)) = remaining.split_once("\"id\"") {
        let Some((_, value)) = after_id.split_once(':') else { break; };
        let value = value.trim_start();
        let Some(value) = value.strip_prefix('"') else { break; };
        let Some(end) = value.find('"') else { break; };
        let id = &value[..end];
        if id.is_empty() || id.contains('\\') { return Vec::new(); }
        ids.push(id.to_owned());
        remaining = &value[end + 1..];
    }
    ids
}

fn json_array_body<'a>(input: &'a str, key: &str) -> Option<&'a str> {
    let marker = format!("\"{key}\"");
    let (_, rest) = input.split_once(&marker)?;
    let rest = rest.trim_start().strip_prefix(':')?.trim_start();
    let start = rest.find('[')?;
    let mut depth = 0usize;
    for (index, byte) in rest.as_bytes().iter().enumerate().skip(start) {
        match byte {
            b'[' => depth += 1,
            b']' => { depth = depth.checked_sub(1)?; if depth == 0 { return Some(&rest[start + 1..index]); } }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn hidden_staging_is_not_discovered() {
        let root = std::env::temp_dir().join(format!("ksub-tweaks-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root); fs::create_dir_all(root.join(".staging")).unwrap();
        assert!(discover(&root, "kindlepw2").unwrap().is_empty());
        let _ = fs::remove_dir_all(root);
    }
    #[test] fn parses_simple_manifest_values() {
        let input = r#"{"id":"com.example.tweak","supported_platforms":["kindlehf","kindlepw2"]}"#;
        assert_eq!(json_string(input, "id").as_deref(), Some("com.example.tweak"));
        assert_eq!(json_string_array(input, "supported_platforms").unwrap().len(), 2);
    }
    #[test] fn extracts_object_dependency_ids() {
        let input = r#"{"dependencies":[{"id":"com.example.base","min":[0,1,0]}]}"#;
        assert_eq!(relationship_ids(input, "dependencies"), vec!["com.example.base"]);
    }
}
