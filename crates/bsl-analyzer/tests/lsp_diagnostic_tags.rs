//! `Diagnostic.tags` against `publishDiagnostics.tagSupport` (github#95): the server
//! sends only the tags a client declared in its `valueSet`, and omits the property
//! entirely when the capability was never announced.

mod common;

use std::path::{Path, PathBuf};

use common::*;
use serde_json::{json, Value};

/// One tagged diagnostic: `Параметр` is unused, so `UnusedParameters` carries
/// `DiagnosticTag::Unnecessary` (wire value `1`). The body is non-empty on purpose —
/// `UnusedParameters` skips empty bodies.
const TAGGED_SOURCE: &str = "Процедура П(Параметр)\r\n    А = 1;\r\nКонецПроцедуры\r\n";

fn tagged_project() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/Main.bsl"), TAGGED_SOURCE).unwrap();
    std::fs::write(dir.path().join("bsl-analyzer.toml"), "[source]\nroot = \"src\"\n").unwrap();
    let path = dir.path().join("src/Main.bsl");
    (dir, path)
}

fn find_unused_parameter(published: &Value) -> &Value {
    published["params"]["diagnostics"]
        .as_array()
        .unwrap_or_else(|| panic!("diagnostics array: {published}"))
        .iter()
        .find(|diagnostic| diagnostic["code"] == "UnusedParameters")
        .unwrap_or_else(|| panic!("no UnusedParameters diagnostic in {published}"))
}

/// Boot a server with `capabilities`, open the tagged module, and return the
/// `UnusedParameters` diagnostic exactly as the client received it.
fn published_diagnostic(root: &Path, path: &Path, capabilities: Value) -> Value {
    let mut lsp = Lsp::start_with_capabilities(root, capabilities);
    let published = lsp.open(path, TAGGED_SOURCE);
    find_unused_parameter(&published).clone()
}

#[test]
fn a_client_without_tag_support_gets_no_tags() {
    let (dir, path) = tagged_project();
    let diagnostic = published_diagnostic(dir.path(), &path, json!({}));
    assert!(
        diagnostic.get("tags").is_none(),
        "a client without tagSupport must not receive the property at all: {diagnostic}"
    );
}

#[test]
fn tags_follow_the_clients_declared_value_set() {
    let (dir, path) = tagged_project();

    // The tag this diagnostic carries is declared: it arrives.
    let declared = published_diagnostic(
        dir.path(),
        &path,
        json!({"textDocument": {"publishDiagnostics": {"tagSupport": {"valueSet": [1]}}}}),
    );
    assert_eq!(declared["tags"], json!([1]), "a declared tag must arrive: {declared}");

    // A partial valueSet that does not include it: filtered out, and the emptied
    // property is dropped rather than sent as an empty array.
    let filtered = published_diagnostic(
        dir.path(),
        &path,
        json!({"textDocument": {"publishDiagnostics": {"tagSupport": {"valueSet": [2]}}}}),
    );
    assert!(
        filtered.get("tags").is_none(),
        "a tag outside the client's valueSet must not be sent: {filtered}"
    );
}
