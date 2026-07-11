use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub image: String,
    pub rva: u64,
    pub prologue: Option<String>,
    pub source: String,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SymbolDb {
    pub firmware: String,
    pub symbols: Vec<Symbol>,
}

impl SymbolDb {
    pub fn lookup(&self, name: &str) -> Option<&Symbol> {
        self.symbols.iter().find(|symbol| symbol.name == name)
    }

    pub fn to_header(&self) -> String {
        let mut output = String::new();
        let guard = format!(
            "KSUB_SYMS_{}",
            self.firmware
                .chars()
                .map(|ch| if ch.is_ascii_alphanumeric() { ch.to_ascii_uppercase() } else { '_' })
                .collect::<String>()
        );
        let _ = writeln!(output, "#ifndef {guard}");
        let _ = writeln!(output, "#define {guard}");
        let _ = writeln!(output);
        let _ = writeln!(output, "typedef struct ksub_symbol_entry {{");
        let _ = writeln!(output, "    const char *name;");
        let _ = writeln!(output, "    const char *image;");
        let _ = writeln!(output, "    unsigned long rva;");
        let _ = writeln!(output, "    const char *prologue;");
        let _ = writeln!(output, "}} ksub_symbol_entry;");
        let _ = writeln!(output);
        let _ = writeln!(output, "static const ksub_symbol_entry KSUB_SYMBOLS[] = {{");
        for symbol in &self.symbols {
            let prologue = symbol.prologue.as_deref().unwrap_or("");
            let _ = writeln!(
                output,
                "    {{\"{}\", \"{}\", 0x{:x}, \"{}\"}},",
                escape_c(&symbol.name),
                escape_c(&symbol.image),
                symbol.rva,
                escape_c(prologue)
            );
        }
        let _ = writeln!(output, "}};");
        let _ = writeln!(output);
        let _ = writeln!(output, "#endif");
        output
    }
}

pub fn parse_symbol_db(input: &str) -> Result<SymbolDb, String> {
    let mut db = SymbolDb::default();
    let mut current: Option<Symbol> = None;

    for raw in input.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(value) = line.strip_prefix("firmware:") {
            db.firmware = unquote(value.trim()).to_owned();
            continue;
        }
        if line == "-" || line == "symbols:" {
            continue;
        }
        if let Some(value) = line.strip_prefix("- name:") {
            if let Some(symbol) = current.take() {
                db.symbols.push(symbol);
            }
            current = Some(Symbol {
                name: unquote(value.trim()).to_owned(),
                image: String::new(),
                rva: 0,
                prologue: None,
                source: "unknown".to_owned(),
            });
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            return Err(format!("invalid symbol db line: {line}"));
        };
        let Some(symbol) = current.as_mut() else {
            continue;
        };
        let value = unquote(value.trim());
        match key.trim() {
            "image" => symbol.image = value.to_owned(),
            "rva" => symbol.rva = parse_u64(value)?,
            "prologue" => symbol.prologue = Some(value.to_owned()),
            "source" => symbol.source = value.to_owned(),
            _ => {}
        }
    }

    if let Some(symbol) = current.take() {
        db.symbols.push(symbol);
    }

    if db.firmware.is_empty() {
        return Err("missing firmware field".to_owned());
    }
    Ok(db)
}

fn parse_u64(value: &str) -> Result<u64, String> {
    if let Some(hex) = value.strip_prefix("0x") {
        u64::from_str_radix(hex, 16).map_err(|error| format!("invalid hex rva {value}: {error}"))
    } else {
        value.parse().map_err(|error| format!("invalid rva {value}: {error}"))
    }
}

fn unquote(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(value)
}

fn escape_c(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_generates_header() {
        let input = r#"
firmware: "5.16.2"
symbols:
  - name: "Reader::openBook"
    image: "/usr/bin/reader"
    rva: 0x1234
    prologue: "00112233"
    source: "manual"
"#;
        let db = parse_symbol_db(input).unwrap();
        assert_eq!(db.lookup("Reader::openBook").unwrap().rva, 0x1234);
        assert!(db.to_header().contains("Reader::openBook"));
    }
}
