use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};

/// Persistent registry owned by Substrate, outside KPM's immutable namespace.
pub const TWEAKS_ROOT: &str = "/var/local/ksubstrate/tweaks";
pub const PACKAGES_ROOT: &str = "/mnt/us/kmc/kpm/packages";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TargetSpec {
    Builtin(String),
    Kpm { package: String, path: String },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Init {
    Constructor,
    Entrypoint,
}
#[derive(Clone, Debug)]
pub struct Manifest {
    pub id: String,
    pub library: String,
    pub initialization: Init,
    pub targets: Vec<TargetSpec>,
    pub dependencies: Vec<String>,
    pub conflicts: Vec<String>,
    pub order: i64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RestartClass {
    Framework,
    NextLaunch,
}
#[derive(Clone, Debug)]
pub struct ResolvedTarget {
    pub id: String,
    pub executable: PathBuf,
    pub restart: RestartClass,
    pub package: Option<String>,
}
#[derive(Clone, Debug)]
pub struct LibraryIdentity {
    pub id: String,
    pub library: PathBuf,
    pub init: Init,
    pub dev: u64,
    pub ino: u64,
    pub size: u64,
    pub digest: u64,
    /// Resolved manifest metadata needed by the runtime loader.  Dependencies
    /// are tweak IDs and are evaluated in the session's deterministic order.
    pub dependencies: Vec<String>,
    pub order: i64,
}
#[derive(Clone, Debug)]
pub struct PlanTarget {
    pub target: ResolvedTarget,
    pub alias: PathBuf,
    pub libraries: Vec<LibraryIdentity>,
}
#[derive(Clone, Debug)]
pub struct SessionPlan {
    pub generation: u64,
    pub platform: String,
    pub targets: Vec<PlanTarget>,
}

/// A package-declared Home entry.  These are intentionally separate from a
/// Substrate tweak manifest: a package can be visible on Home without adding
/// executable launchers to Kindle Documents.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HomeApp {
    pub package_id: String,
    pub app_id: String,
    pub synthetic_id: String,
    pub name: String,
    pub subtitle: Option<String>,
    pub icon: PathBuf,
    pub executable: PathBuf,
    pub arguments: Vec<String>,
    pub working_directory: PathBuf,
}

pub fn platform() -> &'static str {
    if Path::new("/lib/ld-linux-armhf.so.3").exists() {
        "kindlehf"
    } else {
        "kindlepw2"
    }
}
pub fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// Enumerate valid `kindle_home` declarations without writing to either the
/// package tree or Kindle's content catalog. Invalid declarations are skipped
/// individually so a bad third-party package cannot prevent Home from loading.
pub fn discover_home_apps() -> Result<Vec<HomeApp>, String> {
    discover_home_apps_at(Path::new(PACKAGES_ROOT), platform())
}

pub fn discover_home_apps_at(root: &Path, platform: &str) -> Result<Vec<HomeApp>, String> {
    let metadata = match fs::symlink_metadata(root) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("stat KPM package root: {error}")),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("KPM package root is not a real directory".to_owned());
    }
    let mut apps = Vec::new();
    for entry in fs::read_dir(root).map_err(|error| format!("read KPM package root: {error}"))? {
        let Ok(entry) = entry else { continue };
        let directory_id = entry.file_name().to_string_lossy().into_owned();
        if !valid_id(&directory_id) { continue; }
        let package = entry.path();
        let Ok(metadata) = fs::symlink_metadata(&package) else { continue };
        if metadata.file_type().is_symlink() || !metadata.is_dir() { continue; }
        let manifest_path = package.join("manifest.json");
        if regular_file(&manifest_path).is_err() { continue; }
        let Ok(text) = fs::read_to_string(manifest_path) else { continue; };
        if let Ok(package_apps) = parse_home_apps(&text, &package, &directory_id, platform) {
            apps.extend(package_apps);
        }
    }
    apps.sort_by_key(|app| (app.name.to_lowercase(), app.package_id.clone(), app.app_id.clone()));
    Ok(apps)
}

fn parse_home_apps(input: &str, package: &Path, directory_id: &str, platform: &str) -> Result<Vec<HomeApp>, String> {
    let value: Value = serde_json::from_str(input).map_err(|error| format!("invalid package JSON: {error}"))?;
    let object = value.as_object().ok_or_else(|| "package manifest must be a JSON object".to_owned())?;
    let package_id = required_string(object, "id")?;
    if package_id != directory_id || !valid_id(&package_id) { return Err("package id does not match package directory".to_owned()); }
    if let Some(supported) = object.get("supported_platforms") {
        let supported = supported.as_array().ok_or_else(|| "supported_platforms must be a string array".to_owned())?;
        if !supported
            .iter()
            .any(|value| value.as_str() == Some("kindleany") || value.as_str() == Some(platform))
        {
            return Err("package does not support this Kindle platform".to_owned());
        }
    }
    let homes = object.get("kindle_home").and_then(Value::as_array).ok_or_else(|| "kindle_home must be an array".to_owned())?;
    let mut id_counts = BTreeMap::<String, usize>::new();
    for home in homes {
        if let Some(app_id) = home.as_object().and_then(|home| home.get("id")).and_then(Value::as_str) {
            *id_counts.entry(app_id.to_owned()).or_default() += 1;
        }
    }
    Ok(homes
        .iter()
        .filter_map(|home| {
            let home = home.as_object()?;
            let app_id = required_string(home, "id").ok()?;
            if !valid_id(&app_id) || id_counts.get(&app_id) != Some(&1) { return None; }
            parse_home_app(home, package, &package_id, app_id, platform).ok()
        })
        .collect())
}

fn parse_home_app(home: &serde_json::Map<String, Value>, package: &Path, package_id: &str, app_id: String, platform: &str) -> Result<HomeApp, String> {
    let name = required_string(home, "name")?;
    if name.is_empty() { return Err("kindle_home name is empty".to_owned()); }
    let subtitle = home.get("subtitle").map(|value| value.as_str().map(str::to_owned).ok_or_else(|| "kindle_home subtitle must be a string".to_owned())).transpose()?;
    let icon = home_relative_file(package, &required_string(home, "icon")?, platform, "icon")?;
    let executable = home_relative_executable(package, &required_string(home, "executable")?, platform)?;
    let working_directory = match home.get("working_directory") {
        None => package.to_path_buf(),
        Some(value) => home_relative_directory(package, value.as_str().ok_or_else(|| "kindle_home working_directory must be a string".to_owned())?, platform)?,
    };
    let arguments = match home.get("arguments") {
        None => Vec::new(),
        Some(value) => value.as_array().ok_or_else(|| "kindle_home arguments must be a string array".to_owned())?.iter().map(|value| value.as_str().map(str::to_owned).ok_or_else(|| "kindle_home arguments must be a string array".to_owned())).collect::<Result<Vec<_>, _>>()?,
    };
    Ok(HomeApp { synthetic_id: format!("kpm-app://{package_id}/{app_id}"), package_id: package_id.to_owned(), app_id, name, subtitle, icon, executable, arguments, working_directory })
}

fn expand_home_path(value: &str, platform: &str) -> Result<String, String> {
    let value = value.replace("{platform}", platform);
    if value.contains('{') || !safe_relative(&value) { return Err("unsafe kindle_home relative path".to_owned()); }
    Ok(value)
}

fn home_relative_path(package: &Path, value: &str, platform: &str, kind: &str) -> Result<PathBuf, String> {
    let relative = expand_home_path(value, platform)?;
    let mut path = package.to_path_buf();
    for component in Path::new(&relative).components() {
        let Component::Normal(component) = component else { return Err(format!("unsafe kindle_home {kind} path")); };
        path.push(component);
        let metadata = fs::symlink_metadata(&path).map_err(|error| format!("stat kindle_home {kind}: {error}"))?;
        if metadata.file_type().is_symlink() { return Err(format!("kindle_home {kind} contains a symlink")); }
    }
    Ok(path)
}

fn home_relative_file(package: &Path, value: &str, platform: &str, kind: &str) -> Result<PathBuf, String> {
    let path = home_relative_path(package, value, platform, kind)?;
    regular_file(&path)?;
    Ok(path)
}

fn home_relative_executable(package: &Path, value: &str, platform: &str) -> Result<PathBuf, String> {
    let path = home_relative_file(package, value, platform, "executable")?;
    regular_executable(&path)?;
    reject_blacklisted(&path)?;
    Ok(path)
}

fn home_relative_directory(package: &Path, value: &str, platform: &str) -> Result<PathBuf, String> {
    let path = home_relative_path(package, value, platform, "working directory")?;
    let metadata = fs::symlink_metadata(&path).map_err(|error| format!("stat home app working directory: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() { return Err("kindle_home working directory is not a real directory".to_owned()); }
    Ok(path)
}
pub fn regular_file(path: &Path) -> Result<(), String> {
    let m = fs::symlink_metadata(path).map_err(|e| format!("stat {}: {e}", path.display()))?;
    if m.file_type().is_symlink() || !m.is_file() {
        Err(format!("not a regular file: {}", path.display()))
    } else {
        Ok(())
    }
}

pub fn parse_manifest(input: &str) -> Result<Manifest, String> {
    let value: Value =
        serde_json::from_str(input).map_err(|e| format!("invalid manifest JSON: {e}"))?;
    let object = value
        .as_object()
        .ok_or_else(|| "manifest must be a JSON object".to_owned())?;
    if object.get("manifest_version").and_then(Value::as_u64) != Some(2) {
        return Err("manifest_version 2 is required".to_owned());
    }
    let id = required_string(object, "id")?;
    if !valid_id(&id) {
        return Err("invalid manifest id".to_owned());
    }
    let library = required_string(object, "library")?;
    if !simple_name(&library) {
        return Err("unsafe manifest library".to_owned());
    }
    let initialization = match required_string(object, "initialization")?.as_str() {
        "constructor" => Init::Constructor,
        "entrypoint" => Init::Entrypoint,
        _ => return Err("manifest initialization must be constructor or entrypoint".to_owned()),
    };
    let targets = parse_targets(object.get("targets"))?;
    if targets.is_empty() {
        return Err("manifest has no targets".to_owned());
    }
    if targets
        .iter()
        .enumerate()
        .any(|(index, target)| targets[..index].contains(target))
    {
        return Err("manifest targets contains duplicate entries".to_owned());
    }
    let dependencies = optional_string_array(object, "dependencies")?;
    let conflicts = optional_string_array(object, "conflicts")?;
    if dependencies
        .iter()
        .chain(conflicts.iter())
        .any(|id| !valid_id(id))
    {
        return Err("invalid dependency or conflict id".to_owned());
    }
    if dependencies.iter().any(|dependency| dependency == &id) {
        return Err("manifest cannot depend on itself".to_owned());
    }
    if conflicts.iter().any(|conflict| conflict == &id) {
        return Err("manifest cannot conflict with itself".to_owned());
    }
    if has_duplicates(&dependencies) {
        return Err("manifest dependencies contains duplicate ids".to_owned());
    }
    if has_duplicates(&conflicts) {
        return Err("manifest conflicts contains duplicate ids".to_owned());
    }
    if dependencies
        .iter()
        .any(|dependency| conflicts.contains(dependency))
    {
        return Err("manifest dependency conflicts with itself".to_owned());
    }
    let order = match object.get("order") {
        None => 0,
        Some(value) => value
            .as_i64()
            .ok_or_else(|| "manifest order must be a signed integer".to_owned())?,
    };
    Ok(Manifest {
        id,
        library,
        initialization,
        targets,
        dependencies,
        conflicts,
        order,
    })
}

/// Validate all enabled manifests and return the deterministic load order.
/// Dependency edges always win; among eligible tweaks `(order, id)` is used.
pub fn order_manifests(mut manifests: Vec<Manifest>) -> Result<Vec<Manifest>, String> {
    let mut by_id = BTreeMap::new();
    for manifest in manifests.drain(..) {
        if by_id.insert(manifest.id.clone(), manifest).is_some() {
            return Err("duplicate tweak id".to_owned());
        }
    }
    for manifest in by_id.values() {
        for dependency in &manifest.dependencies {
            if !by_id.contains_key(dependency) {
                return Err(format!(
                    "{} requires missing dependency {dependency}",
                    manifest.id
                ));
            }
        }
        for conflict in &manifest.conflicts {
            if by_id.contains_key(conflict) {
                return Err(format!(
                    "{} conflicts with enabled tweak {conflict}",
                    manifest.id
                ));
            }
        }
    }
    let mut remaining: BTreeMap<String, usize> = by_id
        .iter()
        .map(|(id, m)| (id.clone(), m.dependencies.len()))
        .collect();
    let mut ordered = Vec::new();
    while !remaining.is_empty() {
        let mut ready: Vec<_> = remaining
            .iter()
            .filter(|(_, count)| **count == 0)
            .map(|(id, _)| id.clone())
            .collect();
        ready.sort_by_key(|id| (by_id[id].order, id.clone()));
        let Some(id) = ready.into_iter().next() else {
            return Err("tweak dependency cycle".to_owned());
        };
        remaining.remove(&id);
        ordered.push(by_id.remove(&id).unwrap());
        for (other, count) in &mut remaining {
            if by_id[other]
                .dependencies
                .iter()
                .any(|dependency| dependency == &ordered.last().unwrap().id)
            {
                *count -= 1;
            }
        }
    }
    Ok(ordered)
}

pub fn resolve(spec: &TargetSpec, platform: &str) -> Result<ResolvedTarget, String> {
    match spec {
        TargetSpec::Builtin(name) => builtin(name),
        TargetSpec::Kpm { package, path } => resolve_kpm(package, path, platform),
    }
}
fn builtin(name: &str) -> Result<ResolvedTarget, String> {
    let (path, restart) = match name {
        // `pillow` is the stable public target name. Modern firmware exposes
        // the framework root as the pillowd executable.
        "pillow" => ("/usr/bin/pillowd", RestartClass::Framework),
        "appmgrd" => ("/usr/bin/appmgrd", RestartClass::Framework),
        _ => return Err(format!("unknown built-in target: {name}")),
    };
    reject_blacklisted(Path::new(path))?;
    regular_executable(Path::new(path))?;
    Ok(ResolvedTarget {
        id: format!("builtin:{name}"),
        executable: path.into(),
        restart,
        package: None,
    })
}
fn resolve_kpm(package: &str, relative: &str, platform: &str) -> Result<ResolvedTarget, String> {
    if !valid_id(package) {
        return Err("invalid KPM package id".to_owned());
    }
    let expanded = relative.replace("{platform}", platform);
    if expanded.contains('{') || !safe_relative(&expanded) {
        return Err("unsafe KPM target path".to_owned());
    }
    let root = Path::new(PACKAGES_ROOT).join(package);
    let manifest = root.join("manifest.json");
    let text =
        fs::read_to_string(&manifest).map_err(|e| format!("read target package manifest: {e}"))?;
    let package_manifest: Value =
        serde_json::from_str(&text).map_err(|e| format!("invalid target package manifest: {e}"))?;
    if package_manifest
        .get("ksubstrate_target_lifecycle")
        .and_then(Value::as_bool)
        != Some(true)
    {
        return Err(format!(
            "KPM package {package} has not opted into Substrate target lifecycle"
        ));
    }
    let executable = root.join(&expanded);
    if !executable.starts_with(&root) {
        return Err("KPM target escaped package root".to_owned());
    }
    regular_executable(&executable)?;
    reject_blacklisted(&executable)?;
    Ok(ResolvedTarget {
        id: format!("kpm:{package}:{relative}"),
        executable,
        restart: RestartClass::NextLaunch,
        package: Some(package.to_owned()),
    })
}
fn regular_executable(path: &Path) -> Result<(), String> {
    regular_file(path)?;
    let m = fs::metadata(path).map_err(|e| e.to_string())?;
    if m.mode() & 0o111 == 0 {
        Err(format!("not executable: {}", path.display()))
    } else {
        Ok(())
    }
}
fn safe_relative(value: &str) -> bool {
    let p = Path::new(value);
    !p.is_absolute()
        && !value.is_empty()
        && p.components().all(|c| matches!(c, Component::Normal(_)))
}
fn simple_name(value: &str) -> bool {
    !value.is_empty() && Path::new(value).components().count() == 1 && value != "." && value != ".."
}
pub fn reject_blacklisted(path: &Path) -> Result<(), String> {
    let name = path.file_name().and_then(|v| v.to_str()).unwrap_or("");
    if matches!(
        name,
        "powerd"
            | "sshd"
            | "dbus-daemon"
            | "dbus"
            | "otav3"
            | "otaupd"
            | "mmcqd"
            | "wpa_supplicant"
            | "dhcpd"
            | "ksubstrated"
            | "ksubstrate"
    ) {
        Err(format!("blacklisted target: {}", path.display()))
    } else {
        Ok(())
    }
}

pub fn library_identity(
    id: String,
    library: PathBuf,
    init: Init,
    dependencies: Vec<String>,
    order: i64,
) -> Result<LibraryIdentity, String> {
    regular_file(&library)?;
    let m = fs::metadata(&library).map_err(|e| e.to_string())?;
    let bytes = fs::read(&library).map_err(|e| format!("read tweak library: {e}"))?;
    Ok(LibraryIdentity {
        id,
        library,
        init,
        dev: m.dev(),
        ino: m.ino(),
        size: m.size(),
        digest: fnv64(&bytes),
        dependencies,
        order,
    })
}
pub fn verify_library(identity: &LibraryIdentity) -> Result<(), String> {
    let current = library_identity(
        identity.id.clone(),
        identity.library.clone(),
        identity.init.clone(),
        identity.dependencies.clone(),
        identity.order,
    )?;
    if current.dev == identity.dev
        && current.ino == identity.ino
        && current.size == identity.size
        && current.digest == identity.digest
    {
        Ok(())
    } else {
        Err(format!(
            "tweak library changed since session plan: {}",
            identity.library.display()
        ))
    }
}
fn fnv64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325u64, |hash, b| {
        (hash ^ u64::from(*b)).wrapping_mul(0x100000001b3)
    })
}

pub fn encode_plan(plan: &SessionPlan) -> String {
    let mut out = format!(
        "version\t2\ngeneration\t{}\nplatform\t{}\n",
        plan.generation, plan.platform
    );
    for target in &plan.targets {
        out.push_str(&format!(
            "target\t{}\t{}\t{}\t{}\n",
            target.target.id,
            target.target.executable.display(),
            target.alias.display(),
            match target.target.restart {
                RestartClass::Framework => "framework",
                RestartClass::NextLaunch => "next-launch",
            }
        ));
        for library in &target.libraries {
            out.push_str(&format!(
                "library\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                target.target.id,
                library.id,
                library.library.display(),
                match library.init {
                    Init::Constructor => "constructor",
                    Init::Entrypoint => "entrypoint",
                },
                library.dev,
                library.ino,
                library.size,
                library.digest,
                library.order,
                library.dependencies.join(",")
            ));
        }
    }
    out
}
pub fn decode_plan(input: &str) -> Result<SessionPlan, String> {
    let mut version_seen = false;
    let mut generation = None;
    let mut platform = None;
    let mut targets = Vec::<PlanTarget>::new();
    let mut pending = Vec::<(String, LibraryIdentity)>::new();
    for line in input.lines() {
        let f: Vec<_> = line.split('\t').collect();
        match f.as_slice() {
            ["version", "2"] if !version_seen => version_seen = true,
            ["generation", v] => {
                generation = Some(v.parse().map_err(|_| "invalid plan generation")?)
            }
            ["platform", v] => platform = Some((*v).to_owned()),
            ["target", id, exe, alias, restart] => targets.push(PlanTarget {
                target: ResolvedTarget {
                    id: (*id).to_owned(),
                    executable: PathBuf::from(exe),
                    restart: if *restart == "framework" {
                        RestartClass::Framework
                    } else if *restart == "next-launch" {
                        RestartClass::NextLaunch
                    } else {
                        return Err("invalid plan restart class".to_owned());
                    },
                    package: None,
                },
                alias: PathBuf::from(alias),
                libraries: Vec::new(),
            }),
            ["library", target, id, path, init, dev, ino, size, digest, order, dependencies] => {
                pending.push((
                    (*target).to_owned(),
                    LibraryIdentity {
                        id: (*id).to_owned(),
                        library: PathBuf::from(path),
                        init: if *init == "constructor" {
                            Init::Constructor
                        } else if *init == "entrypoint" {
                            Init::Entrypoint
                        } else {
                            return Err("invalid plan init".to_owned());
                        },
                        dev: dev.parse().map_err(|_| "invalid plan dev")?,
                        ino: ino.parse().map_err(|_| "invalid plan ino")?,
                        size: size.parse().map_err(|_| "invalid plan size")?,
                        digest: digest.parse().map_err(|_| "invalid plan digest")?,
                        order: order.parse().map_err(|_| "invalid plan order")?,
                        dependencies: decode_dependencies(dependencies)?,
                    },
                ))
            }
            _ => return Err("malformed session plan".to_owned()),
        }
    }
    for (target, library) in pending {
        let entry = targets
            .iter_mut()
            .find(|entry| entry.target.id == target)
            .ok_or_else(|| "library references unknown target".to_owned())?;
        if entry
            .libraries
            .iter()
            .any(|existing| existing.id == library.id)
        {
            return Err("duplicate library in plan target".to_owned());
        }
        entry.libraries.push(library);
    }
    if !version_seen {
        return Err("missing or unsupported plan version".to_owned());
    }
    if targets.iter().enumerate().any(|(index, target)| {
        targets[..index]
            .iter()
            .any(|other| other.target.id == target.target.id)
    }) {
        return Err("duplicate target in session plan".to_owned());
    }
    Ok(SessionPlan {
        generation: generation.ok_or_else(|| "missing plan generation".to_owned())?,
        platform: platform.ok_or_else(|| "missing plan platform".to_owned())?,
        targets,
    })
}

fn parse_targets(value: Option<&Value>) -> Result<Vec<TargetSpec>, String> {
    let values = value
        .and_then(Value::as_array)
        .ok_or_else(|| "manifest targets must be an array".to_owned())?;
    values
        .iter()
        .map(|value| match value {
            Value::String(name) => Ok(TargetSpec::Builtin(name.clone())),
            Value::Object(object) => {
                if object.get("kind").and_then(Value::as_str) != Some("kpm") {
                    return Err("unknown target object kind".to_owned());
                }
                Ok(TargetSpec::Kpm {
                    package: required_string(object, "package")?,
                    path: required_string(object, "path")?,
                })
            }
            _ => Err("invalid target entry".to_owned()),
        })
        .collect()
}
fn required_string(object: &serde_json::Map<String, Value>, key: &str) -> Result<String, String> {
    object
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("manifest {key} must be a string"))
}

fn optional_string_array(
    object: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<Vec<String>, String> {
    let Some(value) = object.get(key) else {
        return Ok(Vec::new());
    };
    value
        .as_array()
        .ok_or_else(|| format!("{key} must be a string array"))?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("{key} must be a string array"))
        })
        .collect()
}

fn has_duplicates(values: &[String]) -> bool {
    values.iter().collect::<BTreeSet<_>>().len() != values.len()
}

fn decode_dependencies(value: &str) -> Result<Vec<String>, String> {
    if value.is_empty() {
        return Ok(Vec::new());
    }
    let dependencies = value.split(',').map(str::to_owned).collect::<Vec<_>>();
    if dependencies.iter().any(|id| !valid_id(id)) || has_duplicates(&dependencies) {
        return Err("invalid plan dependencies".to_owned());
    }
    Ok(dependencies)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_v2() {
        let m=parse_manifest(r#"{"manifest_version":2,"id":"com.example.x","library":"tweak.so","initialization":"constructor","targets":["pillow"],"dependencies":["com.example.base"],"order":-4}"#).unwrap();
        assert_eq!(m.order, -4);
        assert_eq!(m.dependencies.len(), 1)
    }
    #[test]
    fn orders_dependencies_before_priority() {
        let a=parse_manifest(r#"{"manifest_version":2,"id":"a","library":"a.so","initialization":"constructor","targets":["pillow"],"order":9}"#).unwrap();
        let b=parse_manifest(r#"{"manifest_version":2,"id":"b","library":"b.so","initialization":"constructor","targets":["pillow"],"dependencies":["a"],"order":-9}"#).unwrap();
        assert_eq!(
            order_manifests(vec![b, a])
                .unwrap()
                .iter()
                .map(|m| m.id.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "b"]
        )
    }
    #[test]
    fn rejects_v1() {
        assert!(parse_manifest(r#"{"manifest_version":1}"#).is_err())
    }
    #[test]
    fn rejects_malformed_optional_metadata_instead_of_defaulting() {
        let base = r#"{"manifest_version":2,"id":"a","library":"a.so","initialization":"constructor","targets":["pillow"]"#;
        assert!(parse_manifest(&format!("{base},\"dependencies\":\"b\"}}")).is_err());
        assert!(parse_manifest(&format!("{base},\"order\":\"-2\"}}")).is_err());
        assert!(parse_manifest(&format!("{base},\"dependencies\":[\"b\",\"b\"]}}")).is_err());
    }
    #[test]
    fn json_strings_are_parsed_correctly() {
        let manifest = parse_manifest(r#"{"manifest_version":2,"id":"com.example.x","library":"tweak.so","initialization":"constructor","targets":["pillow"],"dependencies":["com.example.\u0062ase"]}"#).unwrap();
        assert_eq!(manifest.dependencies, vec!["com.example.base"]);
    }
    #[test]
    fn plan_round_trips() {
        let plan = SessionPlan {
            generation: 7,
            platform: "kindlepw2".to_owned(),
            targets: vec![PlanTarget {
                target: ResolvedTarget {
                    id: "builtin:pillow".to_owned(),
                    executable: "/usr/bin/pillowd".into(),
                    restart: RestartClass::Framework,
                    package: None,
                },
                alias: "/tmp/original/pillow".into(),
                libraries: vec![LibraryIdentity {
                    id: "com.example.x".to_owned(),
                    library: "/tmp/tweak.so".into(),
                    init: Init::Constructor,
                    dev: 1,
                    ino: 2,
                    size: 3,
                    digest: 4,
                    dependencies: vec!["com.example.base".to_owned()],
                    order: -4,
                }],
            }],
        };
        let decoded = decode_plan(&encode_plan(&plan)).unwrap();
        assert_eq!(decoded.generation, 7);
        assert_eq!(decoded.targets[0].libraries[0].digest, 4);
        assert_eq!(
            decoded.targets[0].libraries[0].dependencies,
            vec!["com.example.base"]
        );
        assert_eq!(decoded.targets[0].libraries[0].order, -4)
    }
    #[test]
    fn session_plan_requires_current_version() {
        assert!(decode_plan("generation\t1\nplatform\tkindlepw2\n").is_err());
    }

    #[test]
    fn discovers_valid_fileless_home_app() {
        let root = std::env::temp_dir().join(format!("ksub-home-apps-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let package = root.join("com.example.chess");
        fs::create_dir_all(package.join("assets")).unwrap();
        fs::write(package.join("assets/cover.pgm"), b"P2\n1 1\n1\n0\n").unwrap();
        fs::write(package.join("app.sh"), b"#!/bin/sh\nexit 0\n").unwrap();
        let mut permissions = fs::metadata(package.join("app.sh")).unwrap().permissions();
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(0o755);
        fs::set_permissions(package.join("app.sh"), permissions).unwrap();
        fs::write(package.join("manifest.json"), r#"{
          "id":"com.example.chess",
          "kindle_home":[{"id":"play","name":"Chess","icon":"assets/cover.pgm","executable":"app.sh","arguments":["--quick"]}]
        }"#).unwrap();
        let apps = discover_home_apps_at(&root, "kindlehf").unwrap();
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].synthetic_id, "kpm-app://com.example.chess/play");
        assert_eq!(apps[0].package_id, "com.example.chess");
        assert_eq!(apps[0].app_id, "play");
        assert_eq!(apps[0].arguments, vec!["--quick"]);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn omits_home_app_for_another_platform() {
        let root = std::env::temp_dir().join(format!("ksub-home-platform-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let package = root.join("com.example.hfonly");
        fs::create_dir_all(package.join("assets")).unwrap();
        fs::write(package.join("assets/icon"), b"x").unwrap();
        fs::write(package.join("run"), b"#!/bin/sh\n").unwrap();
        let mut permissions = fs::metadata(package.join("run")).unwrap().permissions();
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(0o755);
        fs::set_permissions(package.join("run"), permissions).unwrap();
        fs::write(package.join("manifest.json"), r#"{
          "id":"com.example.hfonly", "supported_platforms":["kindlehf"],
          "kindle_home":[{"id":"main","name":"HF","icon":"assets/icon","executable":"run"}]
        }"#).unwrap();
        assert!(discover_home_apps_at(&root, "kindlepw2").unwrap().is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn bundled_runtime_home_demo_is_discoverable() {
        let package = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../apps/com.bd452.ksubstrate/package")
            .canonicalize()
            .unwrap();
        let manifest = fs::read_to_string(package.join("manifest.json")).unwrap();
        let apps = parse_home_apps(
            &manifest,
            &package,
            "com.bd452.ksubstrate",
            "kindlehf",
        )
        .unwrap();

        assert_eq!(apps.len(), 2);
        assert_eq!(apps[0].synthetic_id, "kpm-app://com.bd452.ksubstrate/test");
        assert_eq!(apps[0].name, "Kindle Substrate Test");
        assert_eq!(apps[0].arguments, vec!["home-demo"]);
        assert_eq!(apps[0].executable, package.join("app.sh"));
        assert_eq!(apps[0].icon, package.join("assets/home-demo.pgm"));
        assert_eq!(apps[1].synthetic_id, "kpm-app://com.bd452.ksubstrate/status");
        assert_eq!(apps[1].arguments, vec!["home-status"]);
    }

    #[test]
    fn keeps_valid_siblings_and_omits_duplicate_app_ids() {
        let root = std::env::temp_dir().join(format!("ksub-home-multiple-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let package = root.join("com.example.tools");
        fs::create_dir_all(package.join("assets")).unwrap();
        fs::write(package.join("assets/icon"), b"x").unwrap();
        fs::write(package.join("run"), b"#!/bin/sh\n").unwrap();
        let mut permissions = fs::metadata(package.join("run")).unwrap().permissions();
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(0o755);
        fs::set_permissions(package.join("run"), permissions).unwrap();
        fs::write(package.join("manifest.json"), r#"{
          "id":"com.example.tools",
          "kindle_home":[
            {"id":"good","name":"Good","icon":"assets/icon","executable":"run"},
            {"id":"broken","name":"Broken","icon":"assets/missing","executable":"run"},
            {"id":"duplicate","name":"Duplicate A","icon":"assets/icon","executable":"run"},
            {"id":"duplicate","name":"Duplicate B","icon":"assets/icon","executable":"run"}
          ]
        }"#).unwrap();

        let apps = discover_home_apps_at(&root, "kindlehf").unwrap();
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].synthetic_id, "kpm-app://com.example.tools/good");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn rejects_symlinked_home_path_components() {
        let root = std::env::temp_dir().join(format!("ksub-home-symlink-{}", std::process::id()));
        let outside = std::env::temp_dir().join(format!("ksub-home-outside-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
        let package = root.join("com.example.escape");
        fs::create_dir_all(&package).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("icon"), b"x").unwrap();
        std::os::unix::fs::symlink(&outside, package.join("assets")).unwrap();
        fs::write(package.join("run"), b"#!/bin/sh\n").unwrap();
        let mut permissions = fs::metadata(package.join("run")).unwrap().permissions();
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(0o755);
        fs::set_permissions(package.join("run"), permissions).unwrap();
        fs::write(package.join("manifest.json"), r#"{
          "id":"com.example.escape",
          "kindle_home":[{"id":"escape","name":"Escape","icon":"assets/icon","executable":"run"}]
        }"#).unwrap();

        assert!(discover_home_apps_at(&root, "kindlehf").unwrap().is_empty());
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
    }
}
