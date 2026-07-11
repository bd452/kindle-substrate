//! Typed paths for the ephemeral runtime.  Keep raw paths at this boundary: the
//! mount layer never accepts an arbitrary `Path` for a system executable.
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};
use ksubstrate_targets::ResolvedTarget;

/// Substrate-owned root. Do not place runtime state under /var/local/kmc:
/// KPM recursively marks that namespace immutable after its own installation.
pub const RUNTIME_ROOT: &str = "/var/local/ksubstrate/runtime";
pub const MOUNTS_ROOT: &str = "/var/local/ksubstrate/runtime/mounts";
pub const STATE_ROOT: &str = "/var/local/ksubstrate/runtime/state";
pub const WRAPPER_ASSET: &str = "/var/local/ksubstrate/assets/wrapper.sh";

#[derive(Debug, Clone, PartialEq, Eq)] pub struct SystemExecutable(PathBuf);
#[derive(Debug, Clone, PartialEq, Eq)] pub struct OriginalAlias(PathBuf);
#[derive(Debug, Clone, PartialEq, Eq)] pub struct WrapperAsset(PathBuf);
#[derive(Debug, Clone, PartialEq, Eq)] pub struct MountTmpfs(PathBuf);
#[derive(Debug, Clone, PartialEq, Eq)] pub struct StateTmpfs(PathBuf);

impl SystemExecutable {
    pub fn from_resolved(target: &ResolvedTarget) -> Result<Self, String> {
        let path = &target.executable;
        if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir | Component::CurDir)) {
            return Err(format!("target executable must be absolute and normalized: {}", path.display()));
        }
        let metadata = fs::symlink_metadata(path).map_err(|e| format!("stat {}: {e}", path.display()))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
            return Err(format!("not a regular executable: {}", path.display()));
        }
        Ok(Self(path.clone()))
    }
    #[cfg(test)]
    pub fn validate(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref();
        if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir | Component::CurDir)) {
            return Err(format!("system executable must be an absolute normalized path: {}", path.display()));
        }
        let allowed = ["/usr/bin", "/usr/sbin", "/bin", "/sbin"];
        if !allowed.iter().any(|root| path.starts_with(root)) {
            return Err(format!("unsupported executable root: {}", path.display()));
        }
        let name = path.file_name().and_then(|n| n.to_str()).ok_or_else(|| "invalid executable name".to_owned())?;
        if is_blacklisted(name) { return Err(format!("recovery-critical executable is blacklisted: {name}")); }
        let metadata = fs::symlink_metadata(path).map_err(|e| format!("stat {}: {e}", path.display()))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
            return Err(format!("not a regular executable: {}", path.display()));
        }
        Ok(Self(path.to_path_buf()))
    }
    pub fn path(&self) -> &Path { &self.0 }
}
impl OriginalAlias {
    pub fn for_system(mounts: &MountTmpfs, system: &SystemExecutable) -> Self {
        Self(mounts.0.join("original").join(system.path().strip_prefix("/").expect("absolute")))
    }
    pub fn path(&self) -> &Path { &self.0 }
}
impl WrapperAsset {
    pub fn installed() -> Result<Self, String> {
        let path = PathBuf::from(WRAPPER_ASSET);
        let metadata = fs::symlink_metadata(&path).map_err(|e| format!("stat wrapper asset: {e}"))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 { return Err("wrapper asset is not a regular executable".to_owned()); }
        Ok(Self(path))
    }
    pub fn path(&self) -> &Path { &self.0 }
}
impl MountTmpfs { pub fn new() -> Self { Self(MOUNTS_ROOT.into()) } pub fn path(&self) -> &Path { &self.0 } }
impl StateTmpfs { pub fn new() -> Self { Self(STATE_ROOT.into()) } pub fn path(&self) -> &Path { &self.0 } }

#[cfg(test)]
pub fn is_blacklisted(name: &str) -> bool {
    matches!(name, "powerd" | "sshd" | "dbus-daemon" | "dbus" | "otav3" | "otaupd" | "mmcqd" | "wpa_supplicant" | "dhcpd")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn aliases_preserve_full_path() {
        let m = MountTmpfs::new();
        let a = m.path().join("original/usr/bin/pillow");
        let b = m.path().join("original/usr/sbin/pillow");
        assert_ne!(a, b);
    }
    #[test] fn rejects_unsafe_paths_before_stat() {
        assert!(SystemExecutable::validate("relative").is_err());
        assert!(SystemExecutable::validate("/tmp/tool").is_err());
        assert!(SystemExecutable::validate("/usr/bin/../pillow").is_err());
        assert!(SystemExecutable::validate("/usr/bin/sshd").is_err());
    }
}
