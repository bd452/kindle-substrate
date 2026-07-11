use ksubstrate_targets::{
    self as targets, decode_plan, library_identity, order_manifests, parse_manifest, resolve,
    Manifest, PlanTarget, SessionPlan,
};
use std::env;
use std::fs;
use std::io::Write;
use std::os::fd::FromRawFd;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const STATE_PLAN: &str = "/var/local/ksubstrate/runtime/state/session.plan";

fn main() {
    kindle_compat::ensure_linked();
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1)
    }
}
fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("run") => run_one_shot(args.collect()),
        Some("wrapped-exec") => wrapped_exec(args.collect()),
        Some("paths") => {
            let paths = Paths::runtime()?;
            println!("package={}", paths.package.display());
            println!("platform={}", paths.platform);
            Ok(())
        }
        Some("help") | Some("--help") | Some("-h") | None => {
            println!(
                "ksubstrate run <program> [args...]\nksubstrate wrapped-exec <path> [args...]"
            );
            Ok(())
        }
        Some(value) => Err(format!("unknown command: {value}")),
    }
}

fn wrapped_exec(args: Vec<String>) -> Result<(), String> {
    let (invoked, rest) = args
        .split_first()
        .ok_or_else(|| "usage: ksubstrate wrapped-exec <path> [args...]".to_owned())?;
    let invoked = PathBuf::from(invoked);
    let plan = decode_plan(
        &fs::read_to_string(STATE_PLAN).map_err(|e| format!("read active session plan: {e}"))?,
    )?;
    let target = plan
        .targets
        .iter()
        .find(|target| target.target.executable == invoked)
        .ok_or_else(|| "invoked path is not an active canonical target".to_owned())?;
    verify_mounts(&target.target.executable, &target.alias)?;
    let paths = Paths::runtime()?;
    exec_preloaded(
        &target.alias,
        rest,
        &paths,
        &target.target.id,
        &target.alias,
        plan.generation,
        None,
    )
}

fn run_one_shot(args: Vec<String>) -> Result<(), String> {
    let (program, rest) = args
        .split_first()
        .ok_or_else(|| "usage: ksubstrate run <program> [args...]".to_owned())?;
    let program = fs::canonicalize(program).map_err(|e| format!("resolve program: {e}"))?;
    let paths = Paths::runtime()?;
    let plan = one_shot_plan(
        &program,
        env::var("KSUBSTRATE_TWEAKS_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(targets::TWEAKS_ROOT)),
    )?;
    let target = plan
        .targets
        .first()
        .ok_or_else(|| "one-shot plan has no target".to_owned())?;
    let fd = plan_pipe(&plan)?;
    exec_preloaded(
        &program,
        rest,
        &paths,
        &target.target.id,
        &program,
        plan.generation,
        Some(fd),
    )
}

fn one_shot_plan(program: &Path, root: PathBuf) -> Result<SessionPlan, String> {
    let mut entries = Vec::<(PathBuf, Manifest)>::new();
    for entry in fs::read_dir(&root).map_err(|e| format!("read tweak registry for run: {e}"))? {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let dir = entry.path();
        entries.push((
            dir.clone(),
            parse_manifest(
                &fs::read_to_string(dir.join("manifest.json"))
                    .map_err(|e| format!("read manifest: {e}"))?,
            )?,
        ));
    }
    let roots = entries
        .iter()
        .map(|(root, manifest)| (manifest.id.clone(), root.clone()))
        .collect::<std::collections::BTreeMap<_, _>>();
    let manifests = order_manifests(entries.into_iter().map(|(_, manifest)| manifest).collect())?;
    let mut targets_out = Vec::new();
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
        for spec in manifest.targets {
            let resolved = resolve(&spec, targets::platform())?;
            if resolved.executable == program {
                if let Some(existing) = targets_out
                    .iter_mut()
                    .find(|target: &&mut PlanTarget| target.target.id == resolved.id)
                {
                    existing.libraries.push(library.clone())
                } else {
                    targets_out.push(PlanTarget {
                        alias: program.to_path_buf(),
                        target: resolved,
                        libraries: vec![library.clone()],
                    })
                }
            }
        }
    }
    if targets_out.is_empty() {
        return Err("program is not an explicit manifest-v2 target".to_owned());
    }
    Ok(SessionPlan {
        generation: 0,
        platform: targets::platform().to_owned(),
        targets: targets_out,
    })
}

fn plan_pipe(plan: &SessionPlan) -> Result<i32, String> {
    let mut fds = [0; 2];
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return Err(format!(
            "create one-shot plan pipe: {}",
            std::io::Error::last_os_error()
        ));
    }
    let mut writer = unsafe { fs::File::from_raw_fd(fds[1]) };
    writer
        .write_all(targets::encode_plan(plan).as_bytes())
        .map_err(|e| format!("write one-shot plan: {e}"))?;
    drop(writer);
    Ok(fds[0])
}

fn exec_preloaded(
    program: &Path,
    args: &[String],
    paths: &Paths,
    target: &str,
    expected: &Path,
    generation: u64,
    one_shot: Option<i32>,
) -> Result<(), String> {
    let mut preload = paths.bootstrap.to_string_lossy().into_owned();
    if let Ok(existing) = env::var("LD_PRELOAD") {
        if !existing.trim().is_empty() {
            preload.push(' ');
            preload.push_str(&existing)
        }
    }
    let lib = paths
        .bootstrap
        .parent()
        .ok_or_else(|| "bootstrap has no parent".to_owned())?;
    let mut command = Command::new(program);
    command
        .args(args)
        .env("LD_PRELOAD", preload)
        .env("LD_LIBRARY_PATH", lib)
        .env("KSUBSTRATE_TARGET", target)
        .env("KSUBSTRATE_EXPECTED_EXE", expected)
        .env("KSUBSTRATE_SESSION_GENERATION", generation.to_string())
        .env("KSUBSTRATE_TWEAKS_DIR", targets::TWEAKS_ROOT);
    if let Some(fd) = one_shot {
        command.env("KSUBSTRATE_ONESHOT_PLAN_FD", fd.to_string());
    } else {
        command.env(
            "KSUBSTRATE_LOG",
            "/var/local/ksubstrate/runtime/state/log/tweaks.log",
        );
    }
    #[cfg(unix)]
    {
        let error = command.exec();
        Err(format!("exec {}: {error}", program.display()))
    }
    #[cfg(not(unix))]
    {
        let status = command.status().map_err(|e| e.to_string())?;
        std::process::exit(status.code().unwrap_or(1))
    }
}

fn verify_mounts(executable: &Path, alias: &Path) -> Result<(), String> {
    let input =
        fs::read_to_string("/proc/self/mountinfo").map_err(|e| format!("read mountinfo: {e}"))?;
    for path in [executable, alias] {
        let entry = input
            .lines()
            .find(|line| {
                line.split_whitespace()
                    .nth(4)
                    .is_some_and(|point| point == path.to_string_lossy())
            })
            .ok_or_else(|| format!("missing bind mount {}", path.display()))?;
        let options = entry.split_whitespace().nth(5).unwrap_or("");
        if !options.split(',').any(|value| value == "ro") {
            return Err(format!("bind mount is not read-only: {}", path.display()));
        }
    }
    Ok(())
}

struct Paths {
    package: PathBuf,
    platform: String,
    bootstrap: PathBuf,
}
impl Paths {
    fn runtime() -> Result<Self, String> {
        let package = PathBuf::from("/mnt/us/kmc/kpm/packages/com.bd452.ksubstrate");
        let platform = targets::platform().to_owned();
        let bootstrap = package
            .join("lib")
            .join(&platform)
            .join("libksubstrate-bootstrap.so");
        if !bootstrap.is_file() {
            return Err(format!("bootstrap not found at {}", bootstrap.display()));
        }
        Ok(Self {
            package,
            platform,
            bootstrap,
        })
    }
}
