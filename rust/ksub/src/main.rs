use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const SDK_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("new") => command_new(args.collect()),
        Some("build") => command_build(args.collect()),
        Some("deploy") => command_deploy(args.collect()),
        Some("package") => command_package(args.collect()),
        Some("pull") => command_pull(args.collect()),
        Some("analyze") => command_analyze(args.collect()),
        Some("sym") => command_sym(args.collect()),
        Some("help") | Some("--help") | Some("-h") | None => {
            print_help();
            Ok(())
        }
        Some(other) => Err(format!("unknown ksub command: {other}")),
    }
}

fn command_new(args: Vec<String>) -> Result<(), String> {
    let kind = args.first().map(String::as_str).unwrap_or("tweak");
    let destination = args.get(1).map(String::as_str).unwrap_or("my-tweak");
    let root = Path::new(destination);
    let name = root
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            format!("project destination must end in a valid UTF-8 name: {destination}")
        })?;
    fs::create_dir_all(root.join("src"))
        .map_err(|error| format!("failed to create project: {error}"))?;
    match kind {
        "tweak" => {
            let crate_name = name.replace('-', "_");
            fs::write(
                root.join("Cargo.toml"),
                format!(
                    "[package]\nname = \"{crate_name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\nbuild = \"build.rs\"\n\n[lib]\ncrate-type = [\"cdylib\"]\n"
                ),
            )
            .map_err(|error| format!("failed to write Cargo.toml: {error}"))?;
            fs::write(root.join("build.rs"), TWEAK_BUILD_RS)
                .map_err(|error| format!("failed to write build.rs: {error}"))?;
            fs::write(root.join("src/lib.rs"), SAMPLE_TWEAK)
                .map_err(|error| format!("failed to write source: {error}"))?;
            // KPM package skeleton (A§9.1): a package manifest plus the tweak
            // payload layout the bootstrap expects under tweaks/<id>/.
            let tweak_pkg = root.join("package").join("tweak");
            fs::create_dir_all(&tweak_pkg)
                .map_err(|error| format!("failed to create package skeleton: {error}"))?;
            fs::write(
                root.join("package").join("manifest.json"),
                package_manifest_json(name),
            )
            .map_err(|error| format!("failed to write package manifest: {error}"))?;
            fs::write(tweak_pkg.join("manifest.json"), tweak_manifest_json(name))
                .map_err(|error| format!("failed to write tweak manifest: {error}"))?;
            fs::write(root.join("package/install.sh"), install_script())
                .map_err(|error| format!("failed to write install hook: {error}"))?;
            fs::write(root.join("package/uninstall.sh"), uninstall_script())
                .map_err(|error| format!("failed to write uninstall hook: {error}"))?;
            fs::write(root.join("README.md"), tweak_readme(name))
                .map_err(|error| format!("failed to write README: {error}"))?;
        }
        "library" | "tool" => {
            fs::write(root.join("README.md"), format!("# {name}\n"))
                .map_err(|error| format!("failed to write README: {error}"))?;
        }
        other => return Err(format!("unknown project kind: {other}")),
    }
    println!("created {kind} project at {}", root.display());
    Ok(())
}

fn command_build(args: Vec<String>) -> Result<(), String> {
    let platform = option_value(&args, "--platform").unwrap_or_else(|| "host".to_owned());
    if platform == "host" {
        run_status(Command::new("cargo").arg("build"))
    } else {
        let target = match platform.as_str() {
            "kindlehf" => "armv7-unknown-linux-gnueabihf",
            "kindlepw2" => "armv7-unknown-linux-gnueabi",
            other => return Err(format!("unknown platform: {other}")),
        };
        let prefix = match platform.as_str() {
            "kindlehf" => "arm-kindlehf-linux-gnueabihf",
            "kindlepw2" => "arm-kindlepw2-linux-gnueabi",
            _ => unreachable!(),
        };
        let linker_env = match platform.as_str() {
            "kindlehf" => "CARGO_TARGET_ARMV7_UNKNOWN_LINUX_GNUEABIHF_LINKER",
            "kindlepw2" => "CARGO_TARGET_ARMV7_UNKNOWN_LINUX_GNUEABI_LINKER",
            _ => unreachable!(),
        };
        let tool_root = env::var("KOXTOOLCHAIN_ROOT").unwrap_or_else(|_| "/opt/x-tools".to_owned());
        let tool_bin = PathBuf::from(tool_root)
            .join("x-tools")
            .join(prefix)
            .join("bin");
        let linker = tool_bin.join(format!("{prefix}-gcc"));
        if !linker.is_file() {
            return Err(format!("Kindle cross-linker not found at {}; run inside the Kindle Substrate build container or set KOXTOOLCHAIN_ROOT", linker.display()));
        }
        let sdk_root = env::var("KSUBSTRATE_SDK_ROOT").unwrap_or_else(|_| SDK_ROOT.to_owned());
        let runtime_lib = Path::new(&sdk_root)
            .join("apps/com.bd452.ksubstrate/package/lib")
            .join(&platform);
        if !runtime_lib.join("libksubstrate.so").is_file() {
            return Err(format!("runtime SDK library missing at {}; build the Kindle Substrate runtime package first or set KSUBSTRATE_SDK_ROOT", runtime_lib.display()));
        }
        let old_path = env::var("PATH").unwrap_or_default();
        run_status(
            Command::new("cargo")
                .args(["build", "--release", "--target", target])
                .env("PATH", format!("{}:{old_path}", tool_bin.display()))
                .env(linker_env, linker)
                .env("KSUBSTRATE_LIB_DIR", &runtime_lib),
        )?;
        stage_tweak(&platform, target)
    }
}

fn command_deploy(args: Vec<String>) -> Result<(), String> {
    let destination =
        option_value(&args, "--dest").unwrap_or_else(|| "/mnt/us/kmc/kpm/packages".to_owned());
    let dest = Path::new(&destination);

    let mut copied = 0;
    let dist_dirs = if Path::new("package/manifest.json").is_file() {
        vec!["dist"]
    } else {
        vec![
            "apps/com.bd452.ksubstrate/dist",
            "apps/com.bd452.ksubstratedemo/dist",
        ]
    };
    for dist in dist_dirs {
        let Ok(entries) = fs::read_dir(dist) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("kpkg") {
                continue;
            }
            fs::create_dir_all(dest)
                .map_err(|error| format!("failed to create {destination}: {error}"))?;
            let target = dest.join(path.file_name().expect("kpkg has a file name"));
            fs::copy(&path, &target)
                .map_err(|error| format!("failed to copy {}: {error}", path.display()))?;
            println!("copied {} -> {}", path.display(), target.display());
            copied += 1;
        }
    }

    if copied == 0 {
        println!("no .kpkg artifacts found under apps/*/dist; run `ksub package` first");
        println!(
            "for a device over SSH, copy the .kpkg files to {destination} with your transport"
        );
    }
    Ok(())
}

fn command_package(_args: Vec<String>) -> Result<(), String> {
    if Path::new("package/manifest.json").is_file() {
        for platform in ["kindlehf", "kindlepw2"] {
            if !Path::new("package/lib")
                .join(platform)
                .join("tweak.so")
                .is_file()
            {
                return Err(format!("missing package/lib/{platform}/tweak.so; run `ksub build --platform {platform}` first"));
            }
        }
        fs::create_dir_all("dist").map_err(|error| format!("failed to create dist: {error}"))?;
        let sdk_root = env::var("KSUBSTRATE_SDK_ROOT").unwrap_or_else(|_| SDK_ROOT.to_owned());
        let packer = Path::new(&sdk_root).join("scripts/pack_kpkg.py");
        if !packer.is_file() {
            return Err(format!(
                "package helper missing at {}; set KSUBSTRATE_SDK_ROOT to the Kindle Substrate checkout",
                packer.display()
            ));
        }
        return run_status(Command::new("python3").args([
            packer.to_string_lossy().as_ref(),
            "package",
            "dist",
        ]));
    }
    run_status(Command::new("bash").arg("apps/com.bd452.ksubstrate/build.sh"))?;
    run_status(Command::new("bash").arg("apps/com.bd452.ksubstratedemo/build.sh"))
}

fn command_pull(args: Vec<String>) -> Result<(), String> {
    let out = option_value(&args, "--out").unwrap_or_else(|| "analysis/pulled".to_owned());
    fs::create_dir_all(&out).map_err(|error| format!("failed to create {out}: {error}"))?;
    println!("created acquisition directory {out}");
    println!("copy /usr/bin, /usr/lib, and framework binaries from the device into this directory");
    Ok(())
}

fn command_analyze(args: Vec<String>) -> Result<(), String> {
    let input = args
        .first()
        .cloned()
        .unwrap_or_else(|| "analysis/pulled".to_owned());
    let firmware = option_value(&args, "--firmware").unwrap_or_else(|| "unknown".to_owned());
    fs::create_dir_all("analysis")
        .map_err(|error| format!("failed to create analysis dir: {error}"))?;

    // Extract exported (dynamic) symbols from each ELF in the input dir via
    // `nm -D`. This is the "free ground truth" tier of A§9.2; the Ghidra /
    // fingerprint / naming tiers remain out of scope for v1.
    let mut symbols: Vec<(String, String, u64)> = Vec::new();
    if let Ok(entries) = fs::read_dir(&input) {
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let image = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_owned();
            if let Ok(out) = Command::new("nm")
                .args(["-D", "--defined-only"])
                .arg(&path)
                .output()
            {
                if out.status.success() {
                    let text = String::from_utf8_lossy(&out.stdout);
                    symbols.extend(parse_nm_symbols(&text, &image));
                }
            }
        }
    }

    let output = PathBuf::from("analysis").join(format!("symbols.{firmware}.yaml"));
    if symbols.is_empty() {
        fs::write(
            &output,
            format!(
                "# No ELF exports extracted from {input} (nm unavailable or no binaries).\n# Fill in manually: pull binaries with `ksub pull`, or add RVAs from Ghidra.\nfirmware: \"{firmware}\"\nsymbols:\n  - name: \"example.symbol\"\n    image: \"example-binary\"\n    rva: 0x0\n    prologue: \"\"\n    source: \"template\"\n"
            ),
        )
        .map_err(|error| format!("failed to write {}: {error}", output.display()))?;
        println!("wrote {} (template — no exports found)", output.display());
        return Ok(());
    }

    let mut yaml =
        format!("# Extracted exported symbols from {input}\nfirmware: \"{firmware}\"\nsymbols:\n");
    for (name, image, rva) in &symbols {
        yaml.push_str(&format!(
            "  - name: \"{name}\"\n    image: \"{image}\"\n    rva: 0x{rva:x}\n    prologue: \"\"\n    source: \"nm-dynsym\"\n"
        ));
    }
    fs::write(&output, yaml)
        .map_err(|error| format!("failed to write {}: {error}", output.display()))?;
    println!(
        "wrote {} ({} exported symbols)",
        output.display(),
        symbols.len()
    );
    Ok(())
}

/// Parse `nm -D --defined-only` output into (name, image, rva) triples. Lines are
/// `<hex addr> <type> <name>`; undefined entries have no address and are skipped.
fn parse_nm_symbols(output: &str, image: &str) -> Vec<(String, String, u64)> {
    output
        .lines()
        .filter_map(|line| {
            let mut cols = line.split_whitespace();
            let addr = cols.next()?;
            let kind = cols.next()?;
            let name = cols.next()?;
            // Exported code/data: global/weak text (T/W), read-only (R), data (D).
            if !matches!(kind, "T" | "W" | "R" | "D") {
                return None;
            }
            let rva = u64::from_str_radix(addr, 16).ok()?;
            Some((name.to_owned(), image.to_owned(), rva))
        })
        .collect()
}

fn package_manifest_json(name: &str) -> String {
    format!(
        "{{\n  \"manifest_version\": 1,\n  \"id\": \"com.example.{name}\",\n  \"name\": \"{name}\",\n  \"author\": \"Your Name\",\n  \"description\": \"Kindle Substrate tweak\",\n  \"version\": [0, 1, 0],\n  \"supported_platforms\": [\"kindlehf\", \"kindlepw2\"],\n  \"dependencies\": [{{\n    \"id\": \"com.bd452.ksubstrate\",\n    \"min\": [0, 1, 0]\n  }}]\n}}\n"
    )
}

fn stage_tweak(platform: &str, target: &str) -> Result<(), String> {
    let crate_name = current_crate_name()?;
    let source = Path::new("target")
        .join(target)
        .join("release")
        .join(format!("lib{crate_name}.so"));
    let destination = Path::new("package/lib").join(platform).join("tweak.so");
    fs::create_dir_all(destination.parent().expect("destination has parent"))
        .map_err(|error| format!("failed to create package library directory: {error}"))?;
    fs::copy(&source, &destination)
        .map_err(|error| format!("failed to stage {}: {error}", source.display()))?;
    println!("staged {}", destination.display());
    Ok(())
}

fn current_crate_name() -> Result<String, String> {
    let manifest = fs::read_to_string("Cargo.toml")
        .map_err(|error| format!("failed to read Cargo.toml: {error}"))?;
    let name = manifest.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        (key.trim() == "name").then(|| value.trim().trim_matches('"').replace('-', "_"))
    });
    name.ok_or_else(|| "package name not found in Cargo.toml".to_owned())
}

fn install_script() -> String {
    r#"#!/bin/sh
set -e
PKG="$(CDPATH= cd "$(dirname "$0")" && pwd)"
ID="$(basename "$PKG")"
DEST="/var/local/ksubstrate/tweaks/$ID"
ROOT="/var/local/ksubstrate/tweaks"
if [ -f /lib/ld-linux-armhf.so.3 ]; then PLAT=kindlehf; else PLAT=kindlepw2; fi
test -f "$PKG/lib/$PLAT/tweak.so"
mkdir -p "$ROOT"
STAGE="$ROOT/.$ID.staging.$$"
OLD="$ROOT/.$ID.retired.$$"
mkdir "$STAGE"
cp "$PKG/lib/$PLAT/tweak.so" "$STAGE/tweak.so"
cp "$PKG/tweak/manifest.json" "$STAGE/manifest.json"
test -s "$STAGE/tweak.so" && test -s "$STAGE/manifest.json"
rollback() {
    if [ -e "$OLD" ] && [ ! -e "$DEST" ]; then mv "$OLD" "$DEST" || true; fi
    rm -rf "$STAGE"
}
trap rollback EXIT HUP INT TERM
if [ -e "$DEST" ]; then mv "$DEST" "$OLD"; fi
mv "$STAGE" "$DEST"
trap - EXIT HUP INT TERM
if ! "/mnt/us/kmc/kpm/packages/com.bd452.ksubstrate/app.sh" post-package-change; then
    rm -rf "$DEST"
    [ -e "$OLD" ] && mv "$OLD" "$DEST"
    exit 1
fi
rm -rf "$OLD"
echo "Installed $ID. Active sessions reconciled successfully."
"#
    .to_owned()
}

fn uninstall_script() -> String {
    "#!/bin/sh\nset -e\n[ \"${1:-}\" = upgrade ] && exit 0\nPKG=\"$(CDPATH= cd \"$(dirname \"$0\")\" && pwd)\"\nID=\"$(basename \"$PKG\")\"\nROOT=/var/local/ksubstrate/tweaks\nDEST=\"$ROOT/$ID\"\nRETIRED=\"$ROOT/.$ID.retired.$$\"\nif [ -e \"$DEST\" ]; then mv \"$DEST\" \"$RETIRED\"; fi\nif ! \"/mnt/us/kmc/kpm/packages/com.bd452.ksubstrate/app.sh\" post-package-change; then [ -e \"$RETIRED\" ] && mv \"$RETIRED\" \"$DEST\"; exit 1; fi\nrm -rf \"$RETIRED\"\n".to_owned()
}

fn tweak_readme(name: &str) -> String {
    format!("# {name}\n\nBuild both Kindle ABIs and package the KPM dependency consumer:\n\n```sh\nksub build --platform kindlehf\nksub build --platform kindlepw2\nksub package\n```\n\nRun these inside the Kindle Substrate build container, with the runtime package built first.\n")
}

fn tweak_manifest_json(name: &str) -> String {
    format!(
        "{{\n  \"manifest_version\": 2,\n  \"id\": \"com.example.{name}\",\n  \"name\": \"{name}\",\n  \"version\": [0, 1, 0],\n  \"library\": \"tweak.so\",\n  \"initialization\": \"constructor\",\n  \"targets\": [\"pillow\"]\n}}\n"
    )
}

fn command_sym(args: Vec<String>) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("lookup") => {
            let db_path = args
                .get(1)
                .ok_or_else(|| "usage: ksub sym lookup <db.yaml> <name>".to_owned())?;
            let name = args
                .get(2)
                .ok_or_else(|| "usage: ksub sym lookup <db.yaml> <name>".to_owned())?;
            let input = fs::read_to_string(db_path)
                .map_err(|error| format!("failed to read {db_path}: {error}"))?;
            let db = ksub_syms::parse_symbol_db(&input)?;
            if let Some(symbol) = db.lookup(name) {
                println!("{} {} 0x{:x}", symbol.image, symbol.name, symbol.rva);
            } else {
                return Err(format!("symbol not found: {name}"));
            }
        }
        Some("header") => {
            let db_path = args
                .get(1)
                .ok_or_else(|| "usage: ksub sym header <db.yaml>".to_owned())?;
            let input = fs::read_to_string(db_path)
                .map_err(|error| format!("failed to read {db_path}: {error}"))?;
            let db = ksub_syms::parse_symbol_db(&input)?;
            print!("{}", db.to_header());
        }
        Some("propose") | Some("promote") => {
            println!("symbol proposal workflow is file-based in this MVP; edit the YAML DB and use `ksub sym header`");
        }
        _ => return Err("usage: ksub sym lookup|header|propose|promote ...".to_owned()),
    }
    Ok(())
}

fn run_status(command: &mut Command) -> Result<(), String> {
    let status = command
        .status()
        .map_err(|error| format!("failed to run command: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("command failed with {status}"))
    }
}

fn option_value(args: &[String], name: &str) -> Option<String> {
    args.windows(2)
        .find_map(|items| (items[0] == name).then(|| items[1].clone()))
}

fn print_help() {
    println!("ksub new tweak|library|tool <name>");
    println!("ksub build [--platform host|kindlehf|kindlepw2]");
    println!("ksub deploy [--dest <path>]");
    println!("ksub package");
    println!("ksub pull [--out <dir>]");
    println!("ksub analyze [dir]");
    println!("ksub sym lookup|header|propose|promote ...");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_nm_extracts_defined_symbols() {
        let output = "\
0000abcd T Reader__openBook\n\
                 U malloc\n\
00001234 W weak_helper\n\
0000ff00 D some_data\n";
        let symbols = parse_nm_symbols(output, "reader");
        assert_eq!(symbols.len(), 3);
        assert!(symbols.contains(&("Reader__openBook".to_owned(), "reader".to_owned(), 0xabcd)));
        assert!(symbols.iter().all(|(name, _, _)| name != "malloc"));
    }

    #[test]
    fn manifests_are_valid_json_shape() {
        let manifest = package_manifest_json("my-tweak");
        assert!(manifest.contains("\"id\": \"com.example.my-tweak\""));
        assert!(manifest.contains("\"dependencies\": [{"));
        assert!(manifest.contains("\"id\": \"com.bd452.ksubstrate\""));
        assert!(tweak_manifest_json("my-tweak").contains("\"library\": \"tweak.so\""));
    }

    #[test]
    fn destination_name_is_a_valid_crate_name() {
        let destination = Path::new("/tmp/projects/my-tweak");
        let name = destination
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap();
        assert_eq!(name.replace('-', "_"), "my_tweak");
    }

    #[test]
    fn scaffolds_at_an_absolute_destination() {
        let root = env::temp_dir().join(format!("ksub-test-{}", std::process::id()));
        let project = root.join("my-tweak");
        let _ = fs::remove_dir_all(&root);
        command_new(vec![
            "tweak".to_owned(),
            project.to_string_lossy().into_owned(),
        ])
        .unwrap();

        let cargo = fs::read_to_string(project.join("Cargo.toml")).unwrap();
        let manifest = fs::read_to_string(project.join("package/manifest.json")).unwrap();
        assert!(cargo.contains("name = \"my_tweak\""));
        assert!(manifest.contains("\"id\": \"com.example.my-tweak\""));
        assert!(project.join("package/install.sh").is_file());
        assert!(project.join("package/uninstall.sh").is_file());
        assert!(fs::read_to_string(project.join("package/install.sh"))
            .unwrap()
            .contains("/var/local/ksubstrate/tweaks"));
        assert!(fs::read_to_string(project.join("package/uninstall.sh"))
            .unwrap()
            .contains("/var/local/ksubstrate/tweaks"));
        assert!(!project.join("tweak.ksfilter").exists());
        assert!(!project.join("package/tweak/tweak.ksfilter").exists());
        let tweak_manifest = fs::read_to_string(project.join("package/tweak/manifest.json")).unwrap();
        assert!(tweak_manifest.contains("\"manifest_version\": 2"));
        assert!(tweak_manifest.contains("\"targets\": [\"pillow\"]"));

        fs::remove_dir_all(root).unwrap();
    }
}

const TWEAK_BUILD_RS: &str = r#"fn main() {
    let lib = std::env::var("KSUBSTRATE_LIB_DIR")
        .expect("KSUBSTRATE_LIB_DIR must point to the runtime package lib/<platform>");
    println!("cargo:rustc-link-search=native={lib}");
    println!("cargo:rustc-link-lib=dylib=ksubstrate");
}
"#;

const SAMPLE_TWEAK: &str = r#"use std::os::raw::c_char;

#[cfg_attr(target_os = "linux", link_section = ".init_array")]
#[used]
static INIT: extern "C" fn() = init;

extern "C" fn init() {
    unsafe { kh_log(b"hello from a Kindle Substrate tweak\0".as_ptr().cast()) };
}

extern "C" {
    fn kh_log(message: *const c_char);
}
"#;
