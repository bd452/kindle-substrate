//! Short-lived transactional controller.  No process owns an active session:
//! verified tmpfs mounts plus a committed plan are the session state.
use crate::{framework, journal::{Journal, Stage}, layout::{MountTmpfs, OriginalAlias, StateTmpfs, SystemExecutable, WrapperAsset}, mounts, tweaks};
use ksubstrate_targets::{decode_plan, encode_plan, PlanTarget, RestartClass, SessionPlan};
use std::fs;

const PLAN: &str = "session.plan";
const PENDING: &str = "session.pending";
const TARGET_CHANGE: &str = "target-package-change";

pub fn status() -> Result<&'static str, String> {
    let mounts = MountTmpfs::new(); let state = StateTmpfs::new();
    let mounts_live = mounts::is_mountpoint(mounts.path())?; let state_live = mounts::is_mountpoint(state.path())?;
    match (mounts_live, state_live) {
        (false, false) => Ok("disabled"),
        (true, true) => { mounts::verify_tmpfs(mounts.path(), false)?; mounts::verify_tmpfs(state.path(), true)?; if state.path().join(TARGET_CHANGE).is_file() { Ok("transitioning") } else if state.path().join(PLAN).is_file() { Ok("active") } else { Ok("recovery-required") } }
        _ => Ok("recovery-required"),
    }
}

pub fn enable() -> Result<(), String> {
    match status()? { "disabled" => apply(None), "active" => Err("Kindle Substrate session is already active".to_owned()), "recovery-required" => { recover()?; apply(None) }, _ => Err("runtime state is ambiguous; reboot required".to_owned()) }
}
pub fn disable() -> Result<(), String> {
    match status()? { "disabled" => Ok(()), "active" => { let plan = read_plan()?; teardown(&plan, true) }, "recovery-required" => recover(), _ => Err("runtime state is ambiguous; reboot required".to_owned()) }
}
pub fn reframe_if_active() -> Result<bool, String> {
    match status()? { "disabled" => Ok(false), "active" => { let old = read_plan()?; apply(Some(old))?; Ok(true) }, _ => Err("runtime state is ambiguous; reboot required".to_owned()) }
}
pub fn post_package_change() -> Result<bool, String> { reframe_if_active() }
pub fn prepare_target_package_change(package: &str) -> Result<bool, String> {
    if !ksubstrate_targets::valid_id(package) { return Err("invalid target package id".to_owned()); }
    match status()? {
        "disabled" => Ok(false),
        "active" => {
            let plan = read_plan()?;
            let affected: Vec<_> = plan.targets.iter().filter(|target| target.target.id.starts_with(&format!("kpm:{package}:"))).cloned().collect();
            if affected.is_empty() { return Ok(false); }
            unmount_targets(&affected)?;
            fs::write(StateTmpfs::new().path().join(TARGET_CHANGE), package).map_err(|e| format!("record target package change: {e}"))?;
            Ok(true)
        }
        _ => Err("runtime state is ambiguous; reboot required".to_owned()),
    }
}
pub fn finish_target_package_change(package: &str) -> Result<bool, String> {
    let state = StateTmpfs::new();
    if !state.path().join(TARGET_CHANGE).is_file() { return reframe_if_active(); }
    let recorded = fs::read_to_string(state.path().join(TARGET_CHANGE)).map_err(|e| format!("read target change: {e}"))?;
    if recorded != package { return Err("different target package change is already pending".to_owned()); }
    let old = read_plan()?;
    apply(Some(old))?;
    let _ = fs::remove_file(state.path().join(TARGET_CHANGE));
    Ok(true)
}

fn recover() -> Result<(), String> {
    let mounts = MountTmpfs::new(); let state = StateTmpfs::new();
    if !mounts::is_mountpoint(mounts.path())? || !mounts::is_mountpoint(state.path())? { return Err("partial runtime tmpfs state is ambiguous; reboot required".to_owned()); }
    mounts::verify_tmpfs(mounts.path(), false)?; mounts::verify_tmpfs(state.path(), true)?;
    let journal = Journal::new(&state); let progress = journal.progress()?;
    if progress.values().any(|entry| entry.intent.is_some()) { return Err("runtime journal has incomplete transition; reboot required".to_owned()); }
    let text = fs::read_to_string(state.path().join(PENDING)).or_else(|_| fs::read_to_string(state.path().join(PLAN))).map_err(|_| "runtime recovery has no session plan; reboot required".to_owned())?;
    let plan = decode_plan(&text)?;
    teardown(&plan, true)
}

fn apply(old: Option<SessionPlan>) -> Result<(), String> {
    let mounts = MountTmpfs::new(); let state = StateTmpfs::new();
    // Validate the next session before mounting tmpfs or touching the UI. An
    // empty registry is a normal disabled state, not a framework failure.
    let generation = old.as_ref().map(|plan| plan.generation + 1).unwrap_or(1);
    let plan = tweaks::build_plan(&mounts, generation)?;
    if plan.targets.is_empty() {
        return match old {
            Some(old) => teardown(&old, true),
            None => Err("no enabled Substrate targets are installed".to_owned()),
        };
    }
    let old_framework = old.as_ref().is_some_and(needs_framework);
    let new_framework = needs_framework(&plan);
    let mut installed = Vec::new();
    let mut framework_touched = false;
    let result = (|| {
        if old_framework { framework::stop_framework()?; framework_touched = true; }
        if let Some(plan) = &old { teardown_mounts(plan)?; let _ = fs::remove_file(state.path().join(PLAN)); }
        if mounts::is_mountpoint(mounts.path())? { mounts::remount_fresh_mounts_tmpfs(&mounts)?; } else { mounts::mount_runtime_tmpfs(&mounts, &state)?; fs::create_dir(state.path().join("log")).map_err(|e| format!("create state log: {e}"))?; }
        if new_framework && !old_framework { framework::stop_framework()?; framework_touched = true; }
        let journal = Journal::new(&state); journal.clear()?;
        let wrapper = WrapperAsset::installed()?;
        for target in &plan.targets { installed.push(target.clone()); install_target(&journal, &mounts, &wrapper, target)?; }
        write_pending(&state, &plan)?;
        if new_framework { framework::restart_hooked()?; framework::wait_for_framework_health(std::time::Duration::from_secs(20))?; }
        commit_plan(&state)
    })();
    if let Err(error) = result {
        let _ = unmount_targets(&installed);
        let _ = fs::remove_file(state.path().join(PENDING)); let _ = fs::remove_file(state.path().join(PLAN));
        if framework_touched { let _ = framework::restart_stock(); }
        if mounts::is_mountpoint(mounts.path()).unwrap_or(false) { let _ = mounts::umount(mounts.path()); }
        if mounts::is_mountpoint(state.path()).unwrap_or(false) { let _ = mounts::umount(state.path()); }
        return Err(error);
    }
    Ok(())
}

fn install_target(journal: &Journal, mounts: &MountTmpfs, wrapper: &WrapperAsset, target: &PlanTarget) -> Result<(), String> {
    let executable = SystemExecutable::from_resolved(&target.target)?;
    let alias = OriginalAlias::for_system(mounts, &executable);
    if alias.path() != target.alias { return Err("session plan alias mismatch".to_owned()); }
    journal.intent(Stage::PrepareAlias, &target.target.id)?; mounts::create_alias_target(mounts, &alias)?; journal.complete(Stage::PrepareAlias, &target.target.id)?;
    journal.intent(Stage::BindOriginal, &target.target.id)?; mounts::bind_original_readonly(&executable, &alias)?; journal.complete(Stage::BindOriginal, &target.target.id)?;
    journal.intent(Stage::ProtectOriginal, &target.target.id)?; journal.complete(Stage::ProtectOriginal, &target.target.id)?;
    journal.intent(Stage::BindWrapper, &target.target.id)?; mounts::bind_wrapper_readonly(wrapper, &executable)?; journal.complete(Stage::BindWrapper, &target.target.id)?;
    journal.intent(Stage::ProtectWrapper, &target.target.id)?; journal.complete(Stage::ProtectWrapper, &target.target.id)
}

fn teardown(plan: &SessionPlan, unmount_state: bool) -> Result<(), String> {
    let mounts = MountTmpfs::new(); let state = StateTmpfs::new();
    let framework = needs_framework(plan); teardown_mounts(plan)?;
    if framework { framework::restart_stock()?; }
    if mounts::is_mountpoint(mounts.path())? { mounts::umount(mounts.path())?; }
    if unmount_state && mounts::is_mountpoint(state.path())? { mounts::umount(state.path())?; }
    Ok(())
}
fn teardown_mounts(plan: &SessionPlan) -> Result<(), String> { unmount_targets(&plan.targets) }
fn unmount_targets(targets: &[PlanTarget]) -> Result<(), String> { let mut errors=Vec::new(); for target in targets.iter().rev(){let exe=SystemExecutable::from_resolved(&target.target)?;if mounts::is_mountpoint(exe.path())?{if let Err(error)=mounts::umount(exe.path()){errors.push(error)}}if mounts::is_mountpoint(&target.alias)?{if let Err(error)=mounts::umount(&target.alias){errors.push(error)}}}if errors.is_empty(){Ok(())}else{Err(errors.join("; "))} }
fn needs_framework(plan:&SessionPlan)->bool { plan.targets.iter().any(|target| matches!(target.target.restart, RestartClass::Framework)) }
fn read_plan()->Result<SessionPlan,String>{let state=StateTmpfs::new();decode_plan(&fs::read_to_string(state.path().join(PLAN)).map_err(|e|format!("read session plan: {e}"))?)}
fn write_pending(state:&StateTmpfs, plan:&SessionPlan)->Result<(),String>{fs::write(state.path().join(PENDING),encode_plan(plan)).map_err(|e|format!("write pending session plan: {e}"))}
fn commit_plan(state:&StateTmpfs)->Result<(),String>{fs::rename(state.path().join(PENDING),state.path().join(PLAN)).map_err(|e|format!("commit session plan: {e}"))}
