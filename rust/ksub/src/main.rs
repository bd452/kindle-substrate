use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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
    let name = args.get(1).map(String::as_str).unwrap_or("my-tweak");
    let root = Path::new(name);
    fs::create_dir_all(root.join("src")).map_err(|error| format!("failed to create project: {error}"))?;
    match kind {
        "tweak" => {
            fs::write(
                root.join("Cargo.toml"),
                format!(
                    "[package]\nname = \"{}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\ncrate-type = [\"cdylib\"]\n\n[dependencies]\nksubstrate = {{ path = \"../ksubstrate\" }}\n",
                    name.replace('-', "_")
                ),
            )
            .map_err(|error| format!("failed to write Cargo.toml: {error}"))?;
            fs::write(root.join("src/lib.rs"), SAMPLE_TWEAK)
                .map_err(|error| format!("failed to write source: {error}"))?;
            fs::write(root.join("tweak.ksfilter"), "pillow\n")
                .map_err(|error| format!("failed to write filter: {error}"))?;
            // KPM package skeleton (A§9.1): a package manifest plus the tweak
            // payload layout the bootstrap expects under tweaks/<id>/.
            let tweak_pkg = root.join("package").join("tweaks").join(name);
            fs::create_dir_all(&tweak_pkg)
                .map_err(|error| format!("failed to create package skeleton: {error}"))?;
            fs::write(root.join("package").join("manifest.json"), package_manifest_json(name))
                .map_err(|error| format!("failed to write package manifest: {error}"))?;
            fs::write(tweak_pkg.join("manifest.json"), tweak_manifest_json(name))
                .map_err(|error| format!("failed to write tweak manifest: {error}"))?;
            fs::write(tweak_pkg.join("tweak.ksfilter"), "pillow\n")
                .map_err(|error| format!("failed to write packaged filter: {error}"))?;
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
        run_status(Command::new("cargo").args(["build", "-p", "ksubstrate"]))
    } else {
        let target = match platform.as_str() {
            "kindlehf" => "armv7-unknown-linux-gnueabihf",
            "kindlepw2" => "armv7-unknown-linux-gnueabi",
            other => return Err(format!("unknown platform: {other}")),
        };
        run_status(Command::new("cargo").args(["build", "--release", "--target", target]))
    }
}

fn command_deploy(args: Vec<String>) -> Result<(), String> {
    let destination = option_value(&args, "--dest").unwrap_or_else(|| "/mnt/us/kmc/kpm/packages".to_owned());
    let dest = Path::new(&destination);

    let mut copied = 0;
    for dist in ["apps/com.bd452.ksubstrate/dist", "apps/com.bd452.ksubstratedemo/dist"] {
        let Ok(entries) = fs::read_dir(dist) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("kpkg") {
                continue;
            }
            fs::create_dir_all(dest).map_err(|error| format!("failed to create {destination}: {error}"))?;
            let target = dest.join(path.file_name().expect("kpkg has a file name"));
            fs::copy(&path, &target).map_err(|error| format!("failed to copy {}: {error}", path.display()))?;
            println!("copied {} -> {}", path.display(), target.display());
            copied += 1;
        }
    }

    if copied == 0 {
        println!("no .kpkg artifacts found under apps/*/dist; run `ksub package` first");
        println!("for a device over SSH, copy the .kpkg files to {destination} with your transport");
    }
    Ok(())
}

fn command_package(_args: Vec<String>) -> Result<(), String> {
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
    let input = args.first().cloned().unwrap_or_else(|| "analysis/pulled".to_owned());
    let firmware = option_value(&args, "--firmware").unwrap_or_else(|| "unknown".to_owned());
    fs::create_dir_all("analysis").map_err(|error| format!("failed to create analysis dir: {error}"))?;

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
            let image = path.file_name().and_then(|n| n.to_str()).unwrap_or("unknown").to_owned();
            if let Ok(out) = Command::new("nm").args(["-D", "--defined-only"]).arg(&path).output() {
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

    let mut yaml = format!("# Extracted exported symbols from {input}\nfirmware: \"{firmware}\"\nsymbols:\n");
    for (name, image, rva) in &symbols {
        yaml.push_str(&format!(
            "  - name: \"{name}\"\n    image: \"{image}\"\n    rva: 0x{rva:x}\n    prologue: \"\"\n    source: \"nm-dynsym\"\n"
        ));
    }
    fs::write(&output, yaml).map_err(|error| format!("failed to write {}: {error}", output.display()))?;
    println!("wrote {} ({} exported symbols)", output.display(), symbols.len());
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
        "{{\n  \"id\": \"com.example.{name}\",\n  \"name\": \"{name}\",\n  \"version\": \"0.1.0\",\n  \"depends\": [\"com.bd452.ksubstrate\"]\n}}\n"
    )
}

fn tweak_manifest_json(name: &str) -> String {
    format!(
        "{{\n  \"id\": \"com.example.{name}\",\n  \"name\": \"{name}\",\n  \"version\": [0, 1, 0],\n  \"filter\": \"tweak.ksfilter\",\n  \"library\": \"tweak.so\"\n}}\n"
    )
}

fn command_sym(args: Vec<String>) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("lookup") => {
            let db_path = args.get(1).ok_or_else(|| "usage: ksub sym lookup <db.yaml> <name>".to_owned())?;
            let name = args.get(2).ok_or_else(|| "usage: ksub sym lookup <db.yaml> <name>".to_owned())?;
            let input = fs::read_to_string(db_path).map_err(|error| format!("failed to read {db_path}: {error}"))?;
            let db = ksub_syms::parse_symbol_db(&input)?;
            if let Some(symbol) = db.lookup(name) {
                println!("{} {} 0x{:x}", symbol.image, symbol.name, symbol.rva);
            } else {
                return Err(format!("symbol not found: {name}"));
            }
        }
        Some("header") => {
            let db_path = args.get(1).ok_or_else(|| "usage: ksub sym header <db.yaml>".to_owned())?;
            let input = fs::read_to_string(db_path).map_err(|error| format!("failed to read {db_path}: {error}"))?;
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
    let status = command.status().map_err(|error| format!("failed to run command: {error}"))?;
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
        assert!(package_manifest_json("my-tweak").contains("\"id\": \"com.example.my-tweak\""));
        assert!(tweak_manifest_json("my-tweak").contains("\"library\": \"tweak.so\""));
    }
}

const SAMPLE_TWEAK: &str = r#"#[cfg_attr(target_os = "linux", link_section = ".init_array")]
#[used]
static INIT: extern "C" fn() = init;

extern "C" fn init() {
    ksubstrate::log("hello from a Kindle Substrate tweak");
}
"#;
