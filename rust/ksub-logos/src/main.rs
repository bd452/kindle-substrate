use std::env;
use std::fs;
use std::io::{self, Read};

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let input = match env::args().nth(1) {
        Some(path) if path == "--help" || path == "-h" => {
            print_help();
            return Ok(());
        }
        Some(path) => fs::read_to_string(&path).map_err(|error| format!("failed to read {path}: {error}"))?,
        None => {
            let mut buffer = String::new();
            io::stdin()
                .read_to_string(&mut buffer)
                .map_err(|error| format!("failed to read stdin: {error}"))?;
            buffer
        }
    };
    print!("{}", preprocess(&input));
    Ok(())
}

fn print_help() {
    println!("ksub-logos [file]");
    println!("Logos-style preprocessor. Supported markers:");
    println!("  KSYM(\"name\")                 -> kh_find_symbol(0, \"name\")");
    println!("  %hookf(ret, target, args...) -> hook fn + constructor registration");
    println!("  %orig                         -> call the original inside a %hookf body");
    println!("  %ctor {{ %init; }}              -> constructor scaffold");
    println!();
    println!("Supported %hookf form: single-line signature, `type name` params.");
}

fn preprocess(input: &str) -> String {
    let mut output = String::from("#include <ksubstrate.h>\n");
    // KSYM first so a KSYM target inside %hookf is already an expression.
    let expanded = expand_ksym(input);
    let expanded = expand_hookf(&expanded);
    for line in expanded.lines() {
        let transformed = line
            .replace("%ctor", "__attribute__((constructor)) static void ksubstrate_tweak_ctor(void)")
            .replace("%init", "/* ksub-logos init */");
        output.push_str(&transformed);
        output.push('\n');
    }
    output
}

/// `KSYM("Reader::openBook")` -> `kh_find_symbol(0, "Reader::openBook")`.
fn expand_ksym(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if input[i..].starts_with("KSYM(") {
            let open = i + "KSYM".len();
            if let Some(close) = matching_delim(input, open, b'(', b')') {
                let arg = input[open + 1..close].trim();
                out.push_str(&format!("kh_find_symbol(0, {arg})"));
                i = close + 1;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

/// Expand each `%hookf(ret, target, params...) { body }` into an original
/// pointer, a hook function, and a constructor that installs the hook. `%orig`
/// inside the body forwards the captured arguments.
fn expand_hookf(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    let mut index = 0usize;
    while let Some(pos) = rest.find("%hookf") {
        out.push_str(&rest[..pos]);
        let after = &rest[pos + "%hookf".len()..];
        let sig_open = match after.find('(') {
            Some(offset) => offset,
            None => {
                out.push_str("%hookf");
                rest = after;
                continue;
            }
        };
        let abs_open = pos + "%hookf".len() + sig_open;
        let Some(sig_close) = matching_delim(rest, abs_open, b'(', b')') else {
            out.push_str(&rest[pos..]);
            return out;
        };
        let signature = &rest[abs_open + 1..sig_close];
        let body_region = &rest[sig_close + 1..];
        let body_open_rel = match body_region.find('{') {
            Some(offset) => offset,
            None => {
                out.push_str(&rest[pos..=sig_close]);
                rest = body_region;
                continue;
            }
        };
        let abs_body_open = sig_close + 1 + body_open_rel;
        let Some(abs_body_close) = matching_delim(rest, abs_body_open, b'{', b'}') else {
            out.push_str(&rest[pos..]);
            return out;
        };
        let body = &rest[abs_body_open + 1..abs_body_close];

        index += 1;
        out.push_str(&emit_hookf(index, signature, body));
        rest = &rest[abs_body_close + 1..];
    }
    out.push_str(rest);
    out
}

fn emit_hookf(n: usize, signature: &str, body: &str) -> String {
    let parts = split_top_level(signature, ',');
    if parts.len() < 2 {
        return format!("/* ksub-logos: malformed %hookf signature: {signature} */");
    }
    let ret = parts[0].trim();
    let target = parts[1].trim();
    let params: Vec<(String, String)> = parts[2..]
        .iter()
        .filter_map(|param| split_param(param.trim()))
        .collect();

    let param_types = params
        .iter()
        .map(|(ty, _)| ty.clone())
        .collect::<Vec<_>>()
        .join(", ");
    let param_decls = params
        .iter()
        .map(|(ty, name)| format!("{ty} {name}"))
        .collect::<Vec<_>>()
        .join(", ");
    let arg_names = params
        .iter()
        .map(|(_, name)| name.clone())
        .collect::<Vec<_>>()
        .join(", ");

    let orig_call = format!("ksub_orig_{n}({arg_names})");
    let body = body.replace("%orig", &orig_call);

    format!(
        "static {ret} (*ksub_orig_{n})({types});\n\
static {ret} ksub_hook_{n}({decls}) {{{body}}}\n\
__attribute__((constructor)) static void ksub_ctor_{n}(void) {{\n\
    void *ksub_target_{n} = (void *)({target});\n\
    kh_hook_function(ksub_target_{n}, (void *)ksub_hook_{n}, (void **)&ksub_orig_{n});\n\
}}\n",
        ret = ret,
        n = n,
        types = if param_types.is_empty() { "void".to_owned() } else { param_types },
        decls = if param_decls.is_empty() { "void".to_owned() } else { param_decls },
        target = target,
        body = body,
    )
}

/// Split `const char *path` into ("const char *", "path").
fn split_param(param: &str) -> Option<(String, String)> {
    let param = param.trim();
    if param.is_empty() || param == "void" {
        return None;
    }
    let split = param
        .rfind(|c: char| c.is_ascii_alphanumeric() || c == '_')
        .and_then(|end| {
            let start = param[..=end]
                .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .map(|i| i + 1)
                .unwrap_or(0);
            Some((start, end + 1))
        })?;
    let name = param[split.0..split.1].to_owned();
    let ty = param[..split.0].trim().to_owned();
    if ty.is_empty() {
        return None;
    }
    Some((ty, name))
}

/// Split on `delim` at the top nesting level (ignoring commas inside () or "").
fn split_top_level(input: &str, delim: char) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut in_str = false;
    let mut current = String::new();
    for ch in input.chars() {
        match ch {
            '"' => {
                in_str = !in_str;
                current.push(ch);
            }
            '(' | '[' | '{' if !in_str => {
                depth += 1;
                current.push(ch);
            }
            ')' | ']' | '}' if !in_str => {
                depth -= 1;
                current.push(ch);
            }
            c if c == delim && depth == 0 && !in_str => {
                parts.push(current.clone());
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    if !current.trim().is_empty() {
        parts.push(current);
    }
    parts
}

/// Index of the delimiter matching `open_at` (which must be `open`), honoring
/// string literals.
fn matching_delim(input: &str, open_at: usize, open: u8, close: u8) -> Option<usize> {
    let bytes = input.as_bytes();
    if bytes.get(open_at) != Some(&open) {
        return None;
    }
    let mut depth = 0i32;
    let mut in_str = false;
    let mut i = open_at;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => in_str = !in_str,
            b if b == open && !in_str => depth += 1,
            b if b == close && !in_str => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_ctor() {
        let output = preprocess("%ctor {\n%init;\n}");
        assert!(output.contains("__attribute__((constructor))"));
        assert!(output.contains("ksub-logos init"));
    }

    #[test]
    fn expands_ksym() {
        let out = expand_ksym("void *p = KSYM(\"Reader::openBook\");");
        assert_eq!(out, "void *p = kh_find_symbol(0, \"Reader::openBook\");");
    }

    #[test]
    fn expands_hookf_with_orig_forwarding() {
        let input = "%hookf(int, KSYM(\"Reader::openBook\"), void *self, const char *path) {\n    return %orig;\n}\n";
        let out = preprocess(input);
        assert!(out.contains("static int (*ksub_orig_1)(void *, const char *);"));
        assert!(out.contains("static int ksub_hook_1(void * self, const char * path)"));
        assert!(out.contains("return ksub_orig_1(self, path);"));
        assert!(out.contains("kh_hook_function(ksub_target_1, (void *)ksub_hook_1, (void **)&ksub_orig_1);"));
        // KSYM inside the target was expanded before the hookf pass.
        assert!(out.contains("kh_find_symbol(0, \"Reader::openBook\")"));
    }

    #[test]
    fn splits_param_types_and_names() {
        assert_eq!(split_param("const char *path"), Some(("const char *".to_owned(), "path".to_owned())));
        assert_eq!(split_param("int n"), Some(("int".to_owned(), "n".to_owned())));
        assert_eq!(split_param("void"), None);
    }
}
