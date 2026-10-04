use crate::define_metadata;
use crate::metadata::*;
use crate::{BodyContext, Diagnostic, DiagnosticCode};
use hir::LocalRange;
use syntax::{preproc_symbols, SyntaxKind, SyntaxNode};

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Error],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
    clean_code_attribute: CleanCodeAttribute::Intentional,
};

#[inline]
pub fn check_node(node: &SyntaxNode, acc: &mut Vec<Diagnostic<LocalRange>>, ctx: &BodyContext) {
    let code = DiagnosticCode::UnknownPreprocessorSymbol;

    if ctx.is_disabled_with_metadata(code) {
        return;
    }

    if node.kind() != SyntaxKind::PRE_SYMBOL {
        return;
    }

    let text = node.text().to_string();
    if !preproc_symbols::is_known(&text) {
        acc.push(Diagnostic {
            code,
            message: format!("Неизвестный символ препроцессора '{}'", text),
            severity: ctx.severity(code),
            range: LocalRange::of_detached_node(node.text_range()),
            tags: ctx.tags(code),
            fixes: vec![],
        });
    }
}

#[cfg(test)]
#[path = "unknown_preprocessor_symbol_corpus.rs"]
mod corpus;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::check_diagnostics_snapshot_for;
    use expect_test::expect;
    /// Классы входов, выведенные из грамматики условия в разделе 4.8.1.2:
    /// условие есть логическое выражение над символами с операциями
    /// `НЕ`, `И`, `Или`, а инструкция начинается с `#`.
    ///
    /// Проверяются разом: символ из таблицы, написание вне её, оба операнда
    /// булевой операции, собственное выражение у `#ИначеЕсли` и обычный
    /// `Если`, который препроцессором не является.
    #[test]
    fn every_class_of_condition_is_covered() {
        let code = r#"#Если ВебКлиент И Мираж Тогда
#ИначеЕсли НЕ Морок Тогда
#Иначе
#КонецЕсли

Если Мираж Тогда
КонецЕсли;
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UnknownPreprocessorSymbol,
            expect![[r#"
                UnknownPreprocessorSymbol @ 1:19..1:24
                  message: Неизвестный символ препроцессора 'Мираж'
                  severity: Critical
                UnknownPreprocessorSymbol @ 2:15..2:20
                  message: Неизвестный символ препроцессора 'Морок'
                  severity: Critical"#]],
        );
    }

    /// Ни одно написание реестра не диагностируется, и оба написания
    /// каждого символа проверены — русское и английское.
    ///
    /// Обход реестра, а не выборка руками: символ, добавленный завтра,
    /// попадёт под проверку сам. Рядом стоит написание вне реестра, иначе
    /// проверка зелена и у реализации, признающей известным вообще всё.
    #[test]
    fn no_registry_spelling_is_reported() {
        use syntax::preproc_symbols::PreprocSymbolId;

        for &id in PreprocSymbolId::ALL {
            for spelling in id.spellings() {
                for written in [spelling.to_string(), spelling.to_uppercase()] {
                    let code = format!("#Если {written} Тогда\n#КонецЕсли\n");
                    let found = crate::test_utils::check_diagnostics_for(
                        &code,
                        DiagnosticCode::UnknownPreprocessorSymbol,
                    );
                    assert!(
                        found.is_empty(),
                        "{written:?}: написание реестра признано неизвестным"
                    );
                }
            }
        }

        let code = "#Если Мираж Тогда\n#КонецЕсли\n";
        assert_eq!(
            crate::test_utils::check_diagnostics_for(
                code,
                DiagnosticCode::UnknownPreprocessorSymbol
            )
            .len(),
            1,
            "написание вне реестра обязано диагностироваться"
        );
    }

    /// Подчёркивание — допустимый идентификатор, но не символ таблицы, и
    /// диагностируется точно по своему диапазону; известный сосед молчит.
    #[test]
    fn a_bare_underscore_is_reported() {
        let code = "#Если _ Тогда\n#КонецЕсли\n\n#Если НаКлиенте Тогда\n#КонецЕсли\n";
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UnknownPreprocessorSymbol,
            expect![[r#"
                UnknownPreprocessorSymbol @ 1:7..1:8
                  message: Неизвестный символ препроцессора '_'
                  severity: Critical"#]],
        );
    }

    /// Неизвестный операнд виден с любой стороны `И`/`Или`, в русской и
    /// английской записи, а известный операнд той же операции молчит.
    #[test]
    fn an_unknown_operand_is_reported_on_either_side() {
        let code = r#"#Если B2Probe И Сервер Тогда
#КонецЕсли
#Если Клиент Или КонтурТеста Тогда
#КонецЕсли
#If B2Probe Or AtServer Then
#EndIf
#If ThinClient And КонтурТеста Then
#EndIf
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UnknownPreprocessorSymbol,
            expect![[r#"
                UnknownPreprocessorSymbol @ 1:7..1:14
                  message: Неизвестный символ препроцессора 'B2Probe'
                  severity: Critical
                UnknownPreprocessorSymbol @ 3:18..3:29
                  message: Неизвестный символ препроцессора 'КонтурТеста'
                  severity: Critical
                UnknownPreprocessorSymbol @ 5:5..5:12
                  message: Неизвестный символ препроцессора 'B2Probe'
                  severity: Critical
                UnknownPreprocessorSymbol @ 7:20..7:31
                  message: Неизвестный символ препроцессора 'КонтурТеста'
                  severity: Critical"#]],
        );
    }

    /// Отрицание не прячет неизвестный символ и не делает известный
    /// неизвестным; `#ИначеЕсли`/`#ElsIf` проверяют своё выражение сами.
    #[test]
    fn negation_and_elsif_conditions_are_checked() {
        let code = r#"#Если НЕ КонтурТеста Тогда
#ИначеЕсли НЕ ВебКлиент Тогда
#ИначеЕсли B2Probe Тогда
#КонецЕсли
#If Not B2Probe Then
#ElsIf Not MobileClient Then
#ElsIf КонтурТеста Then
#EndIf
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UnknownPreprocessorSymbol,
            expect![[r#"
                UnknownPreprocessorSymbol @ 1:10..1:21
                  message: Неизвестный символ препроцессора 'КонтурТеста'
                  severity: Critical
                UnknownPreprocessorSymbol @ 3:12..3:19
                  message: Неизвестный символ препроцессора 'B2Probe'
                  severity: Critical
                UnknownPreprocessorSymbol @ 5:9..5:16
                  message: Неизвестный символ препроцессора 'B2Probe'
                  severity: Critical
                UnknownPreprocessorSymbol @ 7:8..7:19
                  message: Неизвестный символ препроцессора 'КонтурТеста'
                  severity: Critical"#]],
        );
    }

    /// ОС-символы источник не определяет, и в условии они остаются
    /// неизвестными в любом регистре — тот же ответ, что даёт реестр
    /// остальным потребителям. Известный сосед в той же строке молчит.
    #[test]
    fn os_symbols_are_reported() {
        let code = r#"#Если Linux Или Сервер Тогда
#КонецЕсли
#If WINDOWS Or Client Then
#EndIf
#Если НЕ macos И ТонкийКлиент Тогда
#КонецЕсли
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UnknownPreprocessorSymbol,
            expect![[r#"
                UnknownPreprocessorSymbol @ 1:7..1:12
                  message: Неизвестный символ препроцессора 'Linux'
                  severity: Critical
                UnknownPreprocessorSymbol @ 3:5..3:12
                  message: Неизвестный символ препроцессора 'WINDOWS'
                  severity: Critical
                UnknownPreprocessorSymbol @ 5:10..5:15
                  message: Неизвестный символ препроцессора 'macos'
                  severity: Critical"#]],
        );
    }

    /// Продукция 4.8.1.2 перечисляет `Область`/`КонецОбласти` среди символов,
    /// но тот же раздел называет их инструкциями свёртки текста, а не местом
    /// исполнения. В условии они неизвестны во всех четырёх написаниях.
    #[test]
    fn region_words_in_a_condition_are_reported() {
        let code = r#"#Если Область Или Сервер Тогда
#КонецЕсли
#Если КонецОбласти Тогда
#КонецЕсли
#If Region Or Client Then
#EndIf
#If ENDREGION Then
#EndIf
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UnknownPreprocessorSymbol,
            expect![[r#"
                UnknownPreprocessorSymbol @ 1:7..1:14
                  message: Неизвестный символ препроцессора 'Область'
                  severity: Critical
                UnknownPreprocessorSymbol @ 3:7..3:19
                  message: Неизвестный символ препроцессора 'КонецОбласти'
                  severity: Critical
                UnknownPreprocessorSymbol @ 5:5..5:11
                  message: Неизвестный символ препроцессора 'Region'
                  severity: Critical
                UnknownPreprocessorSymbol @ 7:5..7:14
                  message: Неизвестный символ препроцессора 'ENDREGION'
                  severity: Critical"#]],
        );
    }

    /// Написание, которого раздел 4.8.1.2 не определяет, диагностируется.
    ///
    /// Рядом стоит известный символ, который обязан молчать: без него
    /// проверка зелена и у реализации, не признающей ни одного написания.
    #[test]
    fn a_spelling_absent_from_the_source_is_reported() {
        let code = "#Если Linux Тогда\n#КонецЕсли\n\n#Если Сервер Тогда\n#КонецЕсли\n";
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UnknownPreprocessorSymbol,
            expect![[r#"
                UnknownPreprocessorSymbol @ 1:7..1:12
                  message: Неизвестный символ препроцессора 'Linux'
                  severity: Critical"#]],
        );
    }

    #[test]
    fn test_known_symbols() {
        let code = r#"
#Если Сервер Тогда
#КонецЕсли

#Если НЕ МобильныйАвтономныйСервер Тогда
#КонецЕсли
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UnknownPreprocessorSymbol,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_unknown_symbols() {
        let code = r#"
#Если Нечто Тогда
#КонецЕсли
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UnknownPreprocessorSymbol,
            expect![[r#"
                UnknownPreprocessorSymbol @ 2:7..2:12
                  message: Неизвестный символ препроцессора 'Нечто'
                  severity: Critical"#]],
        );
    }

    #[test]
    fn test_complex_conditions() {
        let code = r#"
#Если Клиент ИЛИ Сервер Тогда
#КонецЕсли

#Если НЕ Сервер И ТонкийКлиент Тогда
#КонецЕсли
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UnknownPreprocessorSymbol,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_english_keywords() {
        let code = r#"
#If Client Then
#EndIf

#If Server Then
#EndIf
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UnknownPreprocessorSymbol,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_mixed_known_and_unknown() {
        let code = r#"
#Если Сервер Тогда
#КонецЕсли

#Если UnknownSymbol Тогда
#КонецЕсли
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UnknownPreprocessorSymbol,
            expect![[r#"
                UnknownPreprocessorSymbol @ 5:7..5:20
                  message: Неизвестный символ препроцессора 'UnknownSymbol'
                  severity: Critical"#]],
        );
    }
}
