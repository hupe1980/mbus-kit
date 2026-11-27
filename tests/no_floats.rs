//! Constitution III, checked by a tool: values are exact, so no floating-point type
//! appears anywhere in the library source.

use std::fs;
use std::path::Path;

fn scan(dir: &Path, hits: &mut Vec<String>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            scan(&path, hits);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let text = fs::read_to_string(&path).unwrap();
            for (n, line) in text.lines().enumerate() {
                let code = line.split("//").next().unwrap_or("");
                let words = code.split(|c: char| !c.is_ascii_alphanumeric() && c != '_');
                if words.into_iter().any(|w| w == "f32" || w == "f64") {
                    hits.push(format!("{}:{}: {line}", path.display(), n + 1));
                }
            }
        }
    }
}

#[test]
fn the_library_contains_no_float_type() {
    let mut hits = Vec::new();
    scan(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut hits);
    assert!(hits.is_empty(), "floats in the library (constitution III):\n{}", hits.join("\n"));
}
