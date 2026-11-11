//! Computes CONTROLLER_CORE_BUILD: a fingerprint of everything compiled into the executable.
//! The GUI and the background core must carry the same fingerprint to talk to each other, so the
//! hash covers only files reachable from the crate roots (module tree plus include_str!/
//! include_bytes!/include! inputs), normalises line endings and mixes in the cargo profile,
//! target and enabled features. Stray, unreferenced files never change it.
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

const TEXT_EXTENSIONS: &[&str] = &[
    "rs", "toml", "lock", "js", "html", "css", "json", "svg", "txt", "md", "nsh", "vdf",
];

fn quoted_after(text: &str, marker: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find(marker) {
        rest = &rest[at + marker.len()..];
        let trimmed = rest.trim_start();
        if let Some(body) = trimmed.strip_prefix('"')
            && let Some(end) = body.find('"')
        {
            found.push(body[..end].to_owned());
        }
    }
    found
}

/// `path = "x"` inside an attribute line (`#[path = "x"]`, `#[cfg_attr(.., path = "x")]`).
fn attr_path(line: &str) -> Option<String> {
    let mut from = 0;
    while let Some(at) = line[from..].find("path") {
        let start = from + at + 4;
        from = start;
        let Some(rest) = line[start..].trim_start().strip_prefix('=') else {
            continue;
        };
        if let Some(body) = rest.trim_start().strip_prefix('"')
            && let Some(len) = body.find('"')
        {
            return Some(body[..len].to_owned());
        }
    }
    None
}

/// Advances the bracket `depth` over an attribute line. Returns the text after the attribute(s)
/// once every bracket is closed, e.g. `mod y;` for `#[path = "x"] mod y;`.
fn scan_attribute<'a>(line: &'a str, depth: &mut i32) -> Option<&'a str> {
    let mut in_string = false;
    let mut escaped = false;
    for (index, ch) in line.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '[' => *depth += 1,
            ']' => *depth = (*depth - 1).max(0),
            c if *depth == 0 && !c.is_whitespace() && c != '#' => return Some(&line[index..]),
            _ => {}
        }
    }
    None
}

fn module_declarations(text: &str) -> Vec<(String, Option<String>)> {
    let mut modules = Vec::new();
    let mut pending_path: Option<String> = None;
    // Bracket depth while inside a (possibly multi-line) attribute.
    let mut depth = 0;
    for line in text.lines() {
        let line = line.trim();
        if depth == 0 && line.starts_with("//") {
            continue;
        }
        if depth > 0 || line.starts_with("#[") || line.starts_with("#![") {
            // Any other attribute (e.g. a multi-line #[cfg(..)]) keeps the pending #[path].
            if let Some(path) = attr_path(line) {
                pending_path = Some(path);
            }
            if let Some(rest) = scan_attribute(line, &mut depth) {
                // `#[path = "x"] mod y;` on a single line.
                if let Some(name) = module_name(rest) {
                    modules.push((name, pending_path.take()));
                } else {
                    pending_path = None;
                }
            }
            continue;
        }
        if let Some(name) = module_name(line) {
            modules.push((name, pending_path.take()));
        } else if !line.is_empty() {
            pending_path = None;
        }
    }
    modules
}

#[cfg(test)]
mod tests {
    use super::module_declarations;

    #[test]
    fn path_survives_other_and_multiline_attributes() {
        let text = "#[path = \"a.rs\"]\n#[cfg(any(\n    windows,\n    unix\n))]\nmod a;\n\
                    #[path = \"b.rs\"] mod b;\nmod c;\n#[path = \"d.rs\"]\nuse x;\nmod d;\n";
        assert_eq!(
            module_declarations(text),
            vec![
                ("a".to_owned(), Some("a.rs".to_owned())),
                ("b".to_owned(), Some("b.rs".to_owned())),
                ("c".to_owned(), None),
                ("d".to_owned(), None),
            ]
        );
    }
}

fn module_name(line: &str) -> Option<String> {
    let mut line = line.trim();
    if let Some(rest) = line.strip_prefix("pub") {
        line = rest.trim_start();
        if line.starts_with('(') {
            line = &line[line.find(')')? + 1..];
        }
    }
    let name = line
        .trim_start()
        .strip_prefix("mod ")?
        .trim()
        .strip_suffix(';')?;
    (!name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'))
        .then(|| name.trim().to_owned())
}

fn collect(file: &Path, seen: &mut BTreeSet<PathBuf>) {
    let file = file.to_path_buf();
    if !file.is_file() || !seen.insert(file.clone()) {
        return;
    }
    let dir = file.parent().unwrap_or(Path::new("")).to_path_buf();
    let is_rs = file.extension().is_some_and(|e| e == "rs");
    if !is_rs {
        return;
    }
    let Ok(text) = fs::read_to_string(&file) else {
        return;
    };
    let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let mod_rs = matches!(stem, "main" | "lib" | "mod");
    let child_dir = if mod_rs { dir.clone() } else { dir.join(stem) };
    for (name, path) in module_declarations(&text) {
        if let Some(path) = path {
            collect(&dir.join(path), seen);
        } else {
            let flat = child_dir.join(format!("{name}.rs"));
            let nested = child_dir.join(&name).join("mod.rs");
            collect(if flat.is_file() { &flat } else { &nested }, seen);
        }
    }
    for marker in ["include_str!(", "include_bytes!(", "include!("] {
        for path in quoted_after(&text, marker) {
            collect(&dir.join(path), seen);
        }
    }
}

fn mix(hash: &mut u64, bytes: impl IntoIterator<Item = u8>) {
    for byte in bytes {
        *hash = (*hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
    }
}

fn main() {
    let mut files = BTreeSet::new();
    for root in [
        "src/main.rs",
        "protocol/src/lib.rs",
        "vendor/scap/src/lib.rs",
        "vendor/libspa/src/lib.rs",
        "Cargo.lock",
        "Cargo.toml",
        "protocol/Cargo.toml",
    ] {
        collect(Path::new(root), &mut files);
    }
    let mut hash = 0xcbf29ce484222325u64;
    // Build configuration: a debug GUI must not attach to a release core and vice versa.
    let mut features: Vec<String> = std::env::vars()
        .filter_map(|(k, _)| k.strip_prefix("CARGO_FEATURE_").map(str::to_owned))
        .collect();
    features.sort();
    for part in [
        std::env::var("PROFILE").unwrap_or_default(),
        std::env::var("TARGET").unwrap_or_default(),
        features.join(","),
    ] {
        mix(&mut hash, part.bytes().chain([0xff]));
    }
    for path in &files {
        println!(
            "cargo:rerun-if-changed={}",
            path.to_string_lossy().replace('\\', "/")
        );
        let text_like = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| TEXT_EXTENSIONS.contains(&e));
        let name = path.to_string_lossy().replace('\\', "/");
        mix(&mut hash, name.bytes().chain([0xff]));
        let raw = fs::read(path).unwrap_or_default();
        if text_like {
            let mut index = 0;
            while index < raw.len() {
                if raw[index] == b'\r' && raw.get(index + 1) == Some(&b'\n') {
                    index += 1;
                    continue;
                }
                mix(&mut hash, [raw[index]]);
                index += 1;
            }
        } else {
            mix(&mut hash, raw);
        }
        mix(&mut hash, [0xfe]);
    }
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rustc-env=CONTROLLER_CORE_BUILD={hash:016x}");
    tauri_build::build()
}
