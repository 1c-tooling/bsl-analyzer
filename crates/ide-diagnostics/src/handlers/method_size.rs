use crate::define_metadata;
use crate::metadata::*;
use crate::{BodyContext, Diagnostic, DiagnosticCode};
use hir::LocalRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 30,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
    clean_code_attribute: CleanCodeAttribute::Adaptable,
};

const DEFAULT_MAX_METHOD_SIZE: i64 = 200;

pub fn check_body(ctx: &BodyContext, acc: &mut Vec<Diagnostic<LocalRange>>) {
    let code = DiagnosticCode::MethodSize;
    if ctx.is_disabled_with_metadata(code) {
        return;
    }

    let max_method_size = ctx.config_int(code, "maxMethodSize", DEFAULT_MAX_METHOD_SIZE) as u32;
    let (Some(decl), Some(name_range)) = (ctx.decl(), ctx.method_name_range()) else {
        return;
    };
    let metrics = ctx.hir_metrics();
    if metrics.size_lines <= max_method_size {
        return;
    }
    acc.push(Diagnostic {
        code,
        message: format!(
            "Длина метода \"{}\" равна {}, что больше установленного лимита в {} строк",
            decl.name.as_str(),
            metrics.size_lines,
            max_method_size
        ),
        severity: ctx.severity(code),
        range: name_range,
        tags: ctx.tags(code),
        fixes: vec![],
    });
}

#[cfg(test)]
mod tests {
    use crate::test_utils::{
        check_diagnostics_snapshot_for, check_hir_diagnostic_with_config, format_diags,
    };
    use crate::{DiagnosticCode, DiagnosticsConfig};
    use expect_test::expect;

    fn make_method_size_code() -> String {
        let mut s = String::new();
        s.push_str("Процедура ПустаяПроцедура()\n\n КонецПроцедуры\n\n");
        s.push_str("Функция ФункцияВОднуСтроку() КонецФункции\n\n");
        s.push_str("Процедура Процедура201Строка()\n\n");
        for _ in 0..202 {
            s.push_str("    А = 0;\n");
        }
        s.push_str("\n КонецПроцедуры\n\n");
        s.push_str("Процедура Процедура200Строк()\n\n");
        for _ in 0..201 {
            s.push_str("    А = 0;\n");
        }
        s.push_str("\n КонецПроцедуры\n\n");
        s.push_str("Функция Функция201Строка()\n\n");
        for _ in 0..202 {
            s.push_str("    А = 0;\n");
        }
        s.push_str("\n КонецФункции\n\n");
        s.push_str("Функция Функция200Строк()\n\n");
        for _ in 0..201 {
            s.push_str("    А = 0;\n");
        }
        s.push_str("\n КонецФункции\n\n");
        s.push_str("Функция А(А=0)\n\n КонецФункции\n");
        s
    }

    fn method_size_config(max: serde_json::Value) -> DiagnosticsConfig {
        let mut config = DiagnosticsConfig::default();
        config
            .parameters
            .insert(DiagnosticCode::MethodSize, serde_json::json!({ "maxMethodSize": max }));
        config
    }

    fn method_size_diagnostics(code: &str, config: DiagnosticsConfig) -> Vec<crate::Diagnostic> {
        use ide_db::base_db::SourceDatabase;

        let (mut db, file_id) = crate::test_utils::create_test_db(code);
        // Fixture::parse normalizes CRLF; restore the exact bytes to exercise file ranges.
        db.set_file_text(file_id, code);
        crate::file_diagnostics(&db, file_id, &config)
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::MethodSize)
            .collect()
    }

    fn method_size_diags(code: &str, max_method_size: i64) -> String {
        let diagnostics = method_size_diagnostics(code, method_size_config(max_method_size.into()));
        format_diags(code, &diagnostics)
    }

    fn assert_method_size_report(
        code: &str,
        config: DiagnosticsConfig,
        name: &str,
        size: u32,
        max: u32,
    ) {
        let diagnostics = method_size_diagnostics(code, config);
        assert_eq!(diagnostics.len(), 1, "{code:?}: {diagnostics:?}");
        let diag = &diagnostics[0];
        assert_eq!(
            diag.message,
            format!(
                "Длина метода \"{name}\" равна {size}, что больше установленного лимита в {max} строк"
            )
        );
        let start = code.find(name).unwrap();
        assert_eq!(u32::from(diag.range.start()) as usize, start);
        assert_eq!(u32::from(diag.range.end()) as usize, start + name.len());
        assert_eq!(diag.severity, crate::Severity::Warning);
        assert!(diag.tags.is_empty());
        assert!(diag.fixes.is_empty());
    }

    #[test]
    fn test_comprehensive() {
        let code = make_method_size_code();
        check_diagnostics_snapshot_for(
            &code,
            DiagnosticCode::MethodSize,
            expect![[r#"
                MethodSize @ 7:11..7:29
                  message: Длина метода "Процедура201Строка" равна 205, что больше установленного лимита в 200 строк
                  severity: Warning
                MethodSize @ 214:11..214:28
                  message: Длина метода "Процедура200Строк" равна 204, что больше установленного лимита в 200 строк
                  severity: Warning
                MethodSize @ 420:9..420:25
                  message: Длина метода "Функция201Строка" равна 205, что больше установленного лимита в 200 строк
                  severity: Warning
                MethodSize @ 627:9..627:24
                  message: Длина метода "Функция200Строк" равна 204, что больше установленного лимита в 200 строк
                  severity: Warning"#]],
        );
    }

    #[test]
    fn test_configure_threshold_20() {
        let code = make_method_size_code();
        let mut config = DiagnosticsConfig::default();
        let mut params = serde_json::Map::new();
        params.insert("maxMethodSize".to_string(), serde_json::Value::Number(20.into()));
        config.parameters.insert(DiagnosticCode::MethodSize, serde_json::Value::Object(params));

        let diagnostics = check_hir_diagnostic_with_config(&code, config, crate::diagnostics);
        let diagnostics: Vec<_> =
            diagnostics.into_iter().filter(|d| d.code == DiagnosticCode::MethodSize).collect();
        expect![[r#"
            MethodSize @ 7:11..7:29
              message: Длина метода "Процедура201Строка" равна 205, что больше установленного лимита в 20 строк
              severity: Warning
            MethodSize @ 214:11..214:28
              message: Длина метода "Процедура200Строк" равна 204, что больше установленного лимита в 20 строк
              severity: Warning
            MethodSize @ 420:9..420:25
              message: Длина метода "Функция201Строка" равна 205, что больше установленного лимита в 20 строк
              severity: Warning
            MethodSize @ 627:9..627:24
              message: Длина метода "Функция200Строк" равна 204, что больше установленного лимита в 20 строк
              severity: Warning"#]].assert_eq(&format_diags(&code, &diagnostics));
    }

    #[test]
    fn test_empty_method() {
        let code = r#"Процедура Пустая()

КонецПроцедуры"#;

        check_diagnostics_snapshot_for(code, DiagnosticCode::MethodSize, expect![[r#""#]]);
    }

    #[test]
    fn test_one_liner() {
        let code = r#"Функция Тест() КонецФункции"#;

        check_diagnostics_snapshot_for(code, DiagnosticCode::MethodSize, expect![[r#""#]]);
    }

    #[test]
    fn test_three_line_method_exceeds_threshold_1() {
        let code = "Процедура Тест()\n    А = 1;\nКонецПроцедуры";
        expect![[r#"
            MethodSize @ 1:11..1:15
              message: Длина метода "Тест" равна 2, что больше установленного лимита в 1 строк
              severity: Warning"#]]
        .assert_eq(&method_size_diags(code, 1));
    }

    #[test]
    fn test_two_line_method_equals_threshold_1() {
        let code = "Процедура Тест()\nКонецПроцедуры";
        expect![[r#""#]].assert_eq(&method_size_diags(code, 1));
    }

    #[test]
    fn method_size_small_spans_and_strict_thresholds() {
        for (start, end) in [
            ("Процедура", "КонецПроцедуры"),
            ("Функция", "КонецФункции"),
            ("Procedure", "EndProcedure"),
            ("Function", "EndFunction"),
        ] {
            for size in 0..=4 {
                let separator = if size == 0 { " ".to_owned() } else { "\n".repeat(size) };
                let code = format!("{start} Test(){separator}{end}");
                for max in [0, 1, size as i64] {
                    let config = method_size_config(max.into());
                    if size as i64 > max {
                        assert_method_size_report(&code, config, "Test", size as u32, max as u32);
                    } else {
                        assert!(
                            method_size_diagnostics(&code, config).is_empty(),
                            "{code:?}/{max}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn method_size_node_range_variations() {
        for (code, size) in [
            ("Procedure Test()\n    A = 1; B = 2;\nEndProcedure", 2),
            ("Procedure Test()\n\n    A = 1;\nEndProcedure", 3),
            ("Procedure Test()\n    // comment\n    A = 1;\nEndProcedure", 3),
            ("&AtServer\nProcedure Test()\n    A = 1;\nEndProcedure", 3),
            ("&НаСервере\nФункция Test()\n    // комментарий\n\nКонецФункции", 4),
        ] {
            for prefix in ["", "// outside\n\n\n"] {
                for suffix in ["", "\n\n// outside\n"] {
                    for newline in ["\n", "\r\n"] {
                        let file = format!("{prefix}{code}{suffix}").replace('\n', newline);
                        assert_method_size_report(
                            &file,
                            method_size_config(1.into()),
                            "Test",
                            size,
                            1,
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn method_size_default_boundary_and_non_integer_fallback() {
        let equal = format!("Procedure Test()\n{}EndProcedure", "    A = 1;\n".repeat(199));
        let over = format!("Procedure Test()\n{}EndProcedure", "    A = 1;\n".repeat(200));
        for config in [
            DiagnosticsConfig::default(),
            method_size_config(200.into()),
            method_size_config(serde_json::Value::Null),
            method_size_config(serde_json::json!("1")),
            method_size_config(serde_json::json!(1.5)),
            method_size_config(serde_json::json!(true)),
            method_size_config(serde_json::json!(u64::MAX)),
        ] {
            assert!(method_size_diagnostics(&equal, config.clone()).is_empty());
            assert_method_size_report(&over, config, "Test", 201, 200);
        }
    }

    #[test]
    fn method_size_threshold_keeps_i64_to_u32_cast() {
        let code = "Procedure Test()\n    A = 1;\nEndProcedure";
        for (configured, effective) in [
            (-1_i64, u32::MAX),
            (-4_294_967_296, 0),
            (-4_294_967_295, 1),
            (-4_294_967_294, 2),
            (4_294_967_296, 0),
            (4_294_967_297, 1),
            (i64::MIN, 0),
            (i64::MAX, u32::MAX),
        ] {
            let config = method_size_config(configured.into());
            if effective < 2 {
                assert_method_size_report(code, config, "Test", 2, effective);
            } else {
                assert!(method_size_diagnostics(code, config).is_empty(), "{configured}");
            }
        }
    }

    #[test]
    fn method_size_disabled_keeps_other_diagnostics() {
        let code = "Procedure Test()\n    A = A;\nEndProcedure";
        let enabled = method_size_config(1.into());
        assert_method_size_report(code, enabled.clone(), "Test", 2, 1);
        let mut disabled = enabled.clone();
        disabled.disabled.push(DiagnosticCode::MethodSize);
        assert!(method_size_diagnostics(code, disabled.clone()).is_empty());
        let before = check_hir_diagnostic_with_config(code, enabled, crate::diagnostics);
        let after = check_hir_diagnostic_with_config(code, disabled, crate::diagnostics);
        assert!(after.iter().any(|d| d.code == DiagnosticCode::SelfAssign));
        let others: Vec<_> =
            before.into_iter().filter(|d| d.code != DiagnosticCode::MethodSize).collect();
        assert_eq!(others, after);
    }
}
