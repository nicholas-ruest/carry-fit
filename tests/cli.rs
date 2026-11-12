use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn example_emits_json_plan() {
    let mut command = Command::cargo_bin("carry-fit").expect("binary");
    command
        .args(["examples/weekend.toml", "--format", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"total_score\""))
        .stdout(predicate::str::contains("\"passport\""));
}

#[test]
fn refuses_to_replace_output_without_force() {
    let directory = tempfile::tempdir().expect("tempdir");
    let output = directory.path().join("list.md");
    std::fs::write(&output, "keep me").expect("seed output");

    let mut command = Command::cargo_bin("carry-fit").expect("binary");
    command
        .args(["examples/weekend.toml", "--format", "markdown", "--output"])
        .arg(&output)
        .assert()
        .failure()
        .stderr(predicate::str::contains("File exists"));
    assert_eq!(std::fs::read_to_string(output).expect("read"), "keep me");
}

#[test]
fn writes_markdown_checklist() {
    let directory = tempfile::tempdir().expect("tempdir");
    let output = directory.path().join("list.md");
    let mut command = Command::cargo_bin("carry-fit").expect("binary");
    command
        .args(["examples/weekend.toml", "--format", "markdown", "--output"])
        .arg(&output)
        .assert()
        .success();
    let content = std::fs::read_to_string(output).expect("read");
    assert!(content.starts_with("# Weekend carry-on packing list"));
    assert!(content.contains("- [ ] **Passport**"));
}
