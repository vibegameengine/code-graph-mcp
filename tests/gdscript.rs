use std::{fs, process::Command};
use tempfile::TempDir;

fn cli(root: &std::path::Path, args: &[&str]) -> String {
    let result = Command::new(env!("CARGO_BIN_EXE_code-graph-mcp"))
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8(result.stdout).unwrap()
}

#[test]
fn gdscript_fresh_repository_index_update_and_delete() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    assert!(Command::new("git")
        .args(["init", "-q"])
        .current_dir(root)
        .status()
        .unwrap()
        .success());
    let file = root.join("logic.gd");
    fs::write(
        root.join("sentinel.ts"),
        "export function retained(): number { return 1; }\n",
    )
    .unwrap();
    let original = "extends Node\nfunc tick() -> void:\n\tcommit_score()\nfunc commit_score() -> void:\n\tpass\n";
    fs::write(&file, original).unwrap();
    cli(root, &["rebuild-index", "--confirm"]);
    let found = cli(
        root,
        &["search", "commit_score", "--language", "gdscript", "--json"],
    );
    assert!(
        found.contains("commit_score") && found.contains("logic.gd"),
        "{found}"
    );
    let graph = cli(root, &["callgraph", "tick", "--json"]);
    assert!(graph.contains("commit_score"), "{graph}");
    fs::write(&file, original.replace("commit_score", "save_score")).unwrap();
    cli(root, &["incremental-index", "--quiet"]);
    let found = cli(
        root,
        &["search", "save_score", "--language", "gdscript", "--json"],
    );
    assert!(found.contains("save_score"), "{found}");
    let removed = cli(
        root,
        &["search", "commit_score", "--language", "gdscript", "--json"],
    );
    let rows: serde_json::Value = serde_json::from_str(&removed).unwrap();
    assert!(
        !rows
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n["name"] == "commit_score"),
        "{removed}"
    );
    fs::remove_file(file).unwrap();
    cli(root, &["incremental-index", "--quiet"]);
    let removed = cli(
        root,
        &["search", "save_score", "--language", "gdscript", "--json"],
    );
    let rows: serde_json::Value = serde_json::from_str(&removed).unwrap();
    assert!(
        !rows
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n["name"] == "save_score"),
        "{removed}"
    );
    assert!(cli(root, &["health-check", "--deep"]).starts_with("OK:"));
}
