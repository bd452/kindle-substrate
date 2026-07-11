use crate::{control, framework, journal::{Journal, RootProgress, Stage}, layout::{MountTmpfs, OriginalAlias, StateTmpfs, SystemExecutable, WrapperAsset}, mounts, tweaks};
use std::collections::BTreeMap;
use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

static TERMINATE: AtomicBool = AtomicBool::new(false);

extern "C" fn request_termination(_: libc::c_int) { TERMINATE.store(true, Ordering::SeqCst); }

fn install_signal_handlers() {
    unsafe {
        libc::signal(libc::SIGTERM, request_termination as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, request_termination as *const () as libc::sighandler_t);
        libc::signal(libc::SIGHUP, request_termination as *const () as libc::sighandler_t);
    }
}

#[derive(Clone)]
struct WrappedRoot { executable: SystemExecutable, alias: OriginalAlias }

pub struct Session {
    mounts: MountTmpfs,
    state: StateTmpfs,
    journal: Journal,
    roots: Vec<WrappedRoot>,
    active: bool,
}

impl Session {
    pub fn start() -> Result<Self, String> {
        let mounts = MountTmpfs::new();
        let state = StateTmpfs::new();
        mounts::mount_runtime_tmpfs(&mounts, &state)?;
        let journal = Journal::new(&state);
        let mut session = Self { mounts, state, journal, roots: Vec::new(), active: false };
        if let Err(error) = fs::create_dir(session.state.path().join("log")).map_err(|e| format!("create state log directory: {e}")) {
            let _ = session.teardown(true);
            return Err(error);
        }
        if let Err(error) = session.arm() {
            let _ = session.teardown(true);
            return Err(error);
        }
        install_signal_handlers();
        Ok(session)
    }

    /// Recover only a journaled, internally consistent crashed session.  This
    /// never removes ordinary files or guesses unknown mounts.
    pub fn recover_crashed_session() -> Result<bool, String> {
        let mounts = MountTmpfs::new();
        let state = StateTmpfs::new();
        let mounts_live = mounts::is_mountpoint(mounts.path())?;
        let state_live = mounts::is_mountpoint(state.path())?;
        if !mounts_live && !state_live { return Ok(false); }
        if !mounts_live || !state_live { return Err("partial runtime tmpfs state is ambiguous; reboot required".to_owned()); }
        mounts::verify_tmpfs(mounts.path(), false)?;
        mounts::verify_tmpfs(state.path(), true)?;
        let journal = Journal::new(&state);
        let progress = journal.progress()?;
        if progress.is_empty() { return Err("active runtime has no journal; reboot required".to_owned()); }
        reconcile_and_unmount(&mounts, &progress)?;
        framework::restart_stock()?;
        journal.clear()?;
        mounts::umount(mounts.path())?;
        mounts::umount(state.path())?;
        Ok(true)
    }

    fn arm(&mut self) -> Result<(), String> {
        let wrapper = WrapperAsset::installed()?;
        let roots = tweaks::roots()?;
        for root in roots {
            if !std::path::Path::new(&root).exists() { continue; }
            let executable = SystemExecutable::validate(&root)?;
            let alias = OriginalAlias::for_system(&self.mounts, &executable);
            self.roots.push(WrappedRoot { executable: executable.clone(), alias: alias.clone() });
            self.transition(Stage::PrepareAlias, &root, || mounts::create_alias_target(&self.mounts, &alias))?;
            self.transition(Stage::BindOriginal, &root, || mounts::bind_original_readonly(&executable, &alias))?;
            self.transition(Stage::ProtectOriginal, &root, || Ok(()))?;
            self.transition(Stage::BindWrapper, &root, || mounts::bind_wrapper_readonly(&wrapper, &executable))?;
            self.transition(Stage::ProtectWrapper, &root, || Ok(()))?;
        }
        if self.roots.is_empty() { return Err("no approved framework roots were available".to_owned()); }
        framework::restart_hooked()?;
        framework::wait_for_framework_health(Duration::from_secs(20))?;
        self.active = true;
        Ok(())
    }

    fn transition(&self, stage: Stage, root: &str, operation: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
        self.journal.intent(stage, root)?;
        operation()?;
        self.journal.complete(stage, root)
    }

    fn reframe(&mut self) -> Result<(), String> {
        if !self.active { return Ok(()); }
        self.teardown(false)?;
        mounts::remount_fresh_mounts_tmpfs(&self.mounts)?;
        self.journal.clear()?;
        self.roots.clear();
        if let Err(error) = self.arm() {
            let _ = self.teardown(false);
            return Err(error);
        }
        Ok(())
    }

    fn teardown(&mut self, unmount_state: bool) -> Result<(), String> {
        let result = unmount_wrapped_roots(&self.roots);
        self.roots.clear();
        self.active = false;
        let stock = framework::restart_stock();
        if unmount_state {
            if mounts::is_mountpoint(self.mounts.path())? { mounts::umount(self.mounts.path())?; }
            if mounts::is_mountpoint(self.state.path())? { mounts::umount(self.state.path())?; }
        }
        result?;
        stock
    }

    pub fn serve(mut self) -> Result<(), String> {
        let listener = control::listen(&self.state)?;
        let mut deferred_reframe = false;
        loop {
            if deferred_reframe {
                deferred_reframe = false;
                if let Err(error) = self.reframe() { return Err(format!("deferred reframe failed: {error}")); }
            }
            if TERMINATE.swap(false, Ordering::SeqCst) {
                self.teardown(false)?;
                break;
            }
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let command = control::receive(&stream);
                    let exit = matches!(command.as_deref(), Ok("disable") | Ok("shutdown"));
                    let result = match command {
                        Ok(command) => match command.as_str() {
                            "status" => Ok(if self.active { "enabled" } else { "disabled" }),
                            "disable" | "shutdown" => match self.teardown(false) { Ok(()) => Ok("disabled"), Err(error) => Err(error) },
                            "reframe" => self.reframe().map(|_| "reframed"),
                            "reframe-if-active" => if self.active { self.reframe().map(|_| "reframed") } else { Ok("disabled") },
                            "reframe-if-active-deferred" => { if self.active { deferred_reframe = true; Ok("queued") } else { Ok("disabled") } },
                            _ => Err("unknown command".to_owned()),
                        },
                        Err(error) => Err(error),
                    };
                    control::respond(&mut stream, result)?;
                    if exit { break; }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => thread::sleep(Duration::from_millis(100)),
                Err(error) => return Err(format!("control accept: {error}")),
            }
        }
        let _ = fs::remove_file(control::socket_path(&self.state));
        self.teardown(true)
    }
}

fn unmount_wrapped_roots(roots: &[WrappedRoot]) -> Result<(), String> {
    let mut errors = Vec::new();
    for root in roots.iter().rev() {
        match mounts::is_mountpoint(root.executable.path()) { Ok(true) => if let Err(error) = mounts::umount(root.executable.path()) { errors.push(error); }, Ok(false) => {}, Err(error) => errors.push(error) }
        match mounts::is_mountpoint(root.alias.path()) { Ok(true) => if let Err(error) = mounts::umount(root.alias.path()) { errors.push(error); }, Ok(false) => {}, Err(error) => errors.push(error) }
    }
    if errors.is_empty() { Ok(()) } else { Err(format!("failed to clean every wrapped root: {}", errors.join("; "))) }
}

fn reconcile_and_unmount(mounts_root: &MountTmpfs, progress: &BTreeMap<String, RootProgress>) -> Result<(), String> {
    if progress.values().any(|state| state.intent.is_some()) {
        return Err("runtime journal contains an incomplete transition; reboot required".to_owned());
    }
    for (root, state) in progress.iter().rev() {
        let executable = SystemExecutable::validate(root)?;
        let alias = OriginalAlias::for_system(mounts_root, &executable);
        if state.completed.contains(&Stage::BindWrapper) {
            if !mounts::is_mountpoint(executable.path())? { return Err(format!("journal says wrapper is mounted for {root}, but mountinfo disagrees; reboot required")); }
            mounts::umount(executable.path())?;
        }
        if state.completed.contains(&Stage::BindOriginal) {
            if !mounts::is_mountpoint(alias.path())? { return Err(format!("journal says original alias is mounted for {root}, but mountinfo disagrees; reboot required")); }
            mounts::umount(alias.path())?;
        }
    }
    Ok(())
}
