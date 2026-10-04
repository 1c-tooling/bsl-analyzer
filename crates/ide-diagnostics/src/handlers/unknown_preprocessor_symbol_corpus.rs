use std::path::PathBuf;

use ide_db::base_db::{RootQueryDb, SourceDatabase, SourceRoot, SourceRootId};
use ide_db::RootDatabaseImpl;
use syntax::SyntaxKind;
use vfs::{FileId, FileSet, VfsPath};

use crate::{Diagnostic, DiagnosticCode, DiagnosticsConfig};

fn database(source: &str) -> (RootDatabaseImpl, FileId) {
    let mut db = RootDatabaseImpl::new();
    let file_id = FileId(0);
    let mut files = FileSet::default();
    files.insert(file_id, VfsPath::new("/test.bsl"));
    db.set_source_root(SourceRootId(0), SourceRoot::new_local(files));
    db.set_file_source_root(file_id, SourceRootId(0));
    // Fixture::parse interprets marker comments and normalizes line endings;
    // corpus coordinates must refer to the bytes read from disk instead.
    db.set_file_text(file_id, source);
    (db, file_id)
}

fn config() -> DiagnosticsConfig {
    DiagnosticsConfig {
        only_enabled: Some(vec![DiagnosticCode::UnknownPreprocessorSymbol]),
        ..DiagnosticsConfig::all_enabled()
    }
}

fn diagnostics(db: &RootDatabaseImpl, file_id: FileId) -> Vec<Diagnostic> {
    crate::file_diagnostics(db, file_id, &config())
}

#[test]
fn file_pipeline_preserves_utf8_offsets_across_bodies_and_non_conditions() {
    let source = concat!(
        "\u{feff}// Кириллица перед методом\r\n",
        "//- /not-a-fixture.bsl\r\n",
        "Процедура Проверить()\r\n",
        "#Если НЕ ВебКлиент Тогда\r\n#ИначеЕсли КонтурТеста Тогда\r\n#КонецЕсли\r\n",
        "Сообщить(\"Linux Windows MacOS\"); // #Если Linux Тогда\r\n",
        "Если КонтурТеста Тогда\r\nКонецЕсли;\r\n",
        "КонецПроцедуры\r\n",
        "#Region Linux\r\n#EndRegion\r\n",
        "#Область Контур\r\n#КонецОбласти\r\n",
        "#If B2Probe Or Server Then\r\n#EndIf\r\n",
    );
    let (db, file_id) = database(source);
    assert_eq!(db.file_text(file_id).as_ref(), source);
    assert!(db.parse(file_id).errors().is_empty());
    let mut found = diagnostics(&db, file_id);
    found.sort_by_key(|d| d.range.start());
    assert_eq!(found.len(), 2, "{found:#?}");
    for (diagnostic, spelling) in found.iter().zip(["КонтурТеста", "B2Probe"]) {
        let start = source.find(spelling).expect("operand is present");
        assert_eq!(usize::from(diagnostic.range.start()), start);
        assert_eq!(usize::from(diagnostic.range.end()), start + spelling.len());
        assert_eq!(diagnostic.code, DiagnosticCode::UnknownPreprocessorSymbol);
        assert_eq!(diagnostic.message, format!("Неизвестный символ препроцессора '{spelling}'"));
        assert_eq!(diagnostic.severity, crate::Severity::Critical);
    }
}

#[test]
fn every_boolean_operator_checks_both_operand_positions() {
    for (if_word, then_word, end_word, known, operators) in [
        ("Если", "Тогда", "КонецЕсли", "Сервер", ["И", "Или"]),
        ("If", "Then", "EndIf", "Server", ["And", "Or"]),
    ] {
        for operator in operators {
            for (left, right) in [("КонтурТеста", known), (known, "КонтурТеста")]
            {
                let source =
                    format!("#{if_word} {left} {operator} {right} {then_word}\n#{end_word}\n");
                let (db, file_id) = database(&source);
                assert!(db.parse(file_id).errors().is_empty(), "{source}");
                let found = diagnostics(&db, file_id);
                assert_eq!(found.len(), 1, "{source}: {found:#?}");
                let start = source.find("КонтурТеста").expect("unknown operand");
                assert_eq!(usize::from(found[0].range.start()), start, "{source}");
                assert_eq!(
                    usize::from(found[0].range.end()),
                    start + "КонтурТеста".len(),
                    "{source}"
                );
                assert_eq!(found[0].code, DiagnosticCode::UnknownPreprocessorSymbol);
                assert_eq!(found[0].message, "Неизвестный символ препроцессора 'КонтурТеста'");
                assert_eq!(found[0].severity, crate::Severity::Critical);
            }
        }
    }
}

#[test]
fn os_and_region_spellings_stay_unknown_after_case_changes() {
    for spelling in ["Linux", "Windows", "MacOS", "Область", "КонецОбласти", "Region", "EndRegion"]
    {
        for written in [spelling.to_owned(), spelling.to_uppercase(), spelling.to_lowercase()] {
            let source = format!("#If {written} Or Server Then\n#EndIf\n");
            let (db, file_id) = database(&source);
            assert!(db.parse(file_id).errors().is_empty(), "{source}");
            let found = diagnostics(&db, file_id);
            assert_eq!(found.len(), 1, "{source}: {found:#?}");
            let start = source.find(&written).expect("unknown operand");
            assert_eq!(usize::from(found[0].range.start()), start, "{source}");
            assert_eq!(usize::from(found[0].range.end()), start + written.len(), "{source}");
            assert_eq!(found[0].code, DiagnosticCode::UnknownPreprocessorSymbol);
            assert_eq!(found[0].message, format!("Неизвестный символ препроцессора '{written}'"));
            assert_eq!(found[0].severity, crate::Severity::Critical);
        }
    }
}

/// Export the production diagnostic tuples for an explicitly supplied corpus.
///
/// BSL_PREPROC_AB_INPUTS is a JSON array of file paths; BSL_PREPROC_AB_OUTPUT
/// names a JSON output file. No external trees are needed for ordinary tests.
#[test]
#[ignore = "requires an explicit corpus manifest and output path"]
fn export_unknown_preprocessor_symbol_corpus() {
    let manifest = std::env::var_os("BSL_PREPROC_AB_INPUTS").expect("BSL_PREPROC_AB_INPUTS");
    let output = std::env::var_os("BSL_PREPROC_AB_OUTPUT").expect("BSL_PREPROC_AB_OUTPUT");
    let paths: Vec<PathBuf> =
        serde_json::from_slice(&std::fs::read(manifest).expect("read corpus manifest"))
            .expect("JSON array of paths");
    assert!(!paths.is_empty(), "an empty corpus proves nothing");
    let mut files = Vec::new();
    let mut records = Vec::new();
    for path in paths {
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let (db, file_id) = database(&source);
        let parse = db.parse(file_id);
        let symbols = parse
            .syntax_node()
            .descendants()
            .filter(|node| node.kind() == SyntaxKind::PRE_SYMBOL)
            .count();
        files.push((path.clone(), source.len(), symbols, parse.errors().len()));
        for diagnostic in diagnostics(&db, file_id) {
            assert_eq!(diagnostic.code, DiagnosticCode::UnknownPreprocessorSymbol);
            records.push((
                path.clone(),
                u32::from(diagnostic.range.start()),
                u32::from(diagnostic.range.end()),
                diagnostic.code.as_str().to_owned(),
                diagnostic.message,
                diagnostic.severity.as_str().to_owned(),
            ));
        }
    }
    files.sort();
    records.sort();
    let result = serde_json::json!({"files": files, "diagnostics": records});
    std::fs::write(output, serde_json::to_vec_pretty(&result).expect("serialize corpus"))
        .expect("write corpus result");
}
