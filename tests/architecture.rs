//! Structural rules that keep lookups offline and source formats out of the core.

use std::fs;
use std::path::Path;

fn sources() -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut stack = vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("src")];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let rel = path
                    .strip_prefix(env!("CARGO_MANIFEST_DIR"))
                    .unwrap()
                    .display()
                    .to_string();
                out.push((rel, fs::read_to_string(&path).unwrap()));
            }
        }
    }
    out
}

#[test]
fn only_net_module_uses_http() {
    for (path, text) in sources() {
        if path == "src/net.rs" {
            continue;
        }
        assert!(
            !text.contains("ureq"),
            "{path} must not use the HTTP client"
        );
    }
}

#[test]
fn only_update_command_reaches_network() {
    for (path, text) in sources() {
        if path != "src/net.rs" && text.contains("Http::new(") {
            assert_eq!(path, "src/app.rs", "{path} constructs an HTTP client");
            let start = text.find("fn update(").expect("update command");
            let end = start + text[start..].find("\n}\n").unwrap();
            let uses: Vec<_> = text.match_indices("Http::new(").map(|(i, _)| i).collect();
            assert!(
                uses.iter().all(|&i| i > start && i < end),
                "Http::new used outside `drug update`"
            );
        }
    }
}

#[test]
fn source_formats_stay_in_adapters() {
    // RxNorm file and field names must not appear outside the adapter.
    for (path, text) in sources() {
        if path.starts_with("src/sources/") {
            continue;
        }
        for word in ["RXNCONSO", "RXNREL", "RXNSAT", "\"SCD\"", "\"TTY\"", "RELA"] {
            assert!(!text.contains(word), "{path} mentions {word}");
        }
    }
}
