//! Projection of documentation source roles onto semantic tokens.

use hir::{doc_comment_tokens, DocCommentTokenKind};
use syntax::{SyntaxNode, TextRange, TextSize};

use super::{HlMod, HlRange, HlTag};

/// Refines the comment suffix already visited before a method, including annotations.
/// Only adjacent full-line comments belong to the method's documentation.
pub(super) fn highlight_leading_comments(
    method: &SyntaxNode,
    source: &str,
    highlights: &mut Vec<HlRange>,
) {
    let mut cursor = usize::from(method.text_range().start());
    let mut first = highlights.len();
    for comment in highlights.iter().rev() {
        if comment.tag != HlTag::Comment {
            break;
        }
        let start = usize::from(comment.range.start());
        let end = usize::from(comment.range.end());
        let Some(gap) = source.get(end..cursor) else { break };
        let line_start = source[..start].rfind('\n').map_or(0, |pos| pos + 1);
        if !gap.chars().all(char::is_whitespace)
            || gap.bytes().filter(|byte| *byte == b'\n').count() != 1
            || !source[line_start..start].trim_start_matches('\u{feff}').trim().is_empty()
        {
            break;
        }
        first -= 1;
        cursor = start;
    }
    if first == highlights.len() {
        return;
    }

    let comments = highlights.split_off(first);
    let lines: Vec<_> = comments
        .iter()
        .map(|comment| {
            &source[usize::from(comment.range.start()) + 2..usize::from(comment.range.end())]
        })
        .collect();
    let mut tokens = doc_comment_tokens(&lines).into_iter().peekable();
    for (index, comment) in comments.into_iter().enumerate() {
        let base = comment.range.start() + TextSize::from(2);
        let mut start = comment.range.start();
        while tokens.peek().is_some_and(|token| token.line == index) {
            let token = tokens.next().unwrap();
            let range = token.range + base;
            push_comment(highlights, TextRange::new(start, range.start()));
            let tag = match token.kind {
                DocCommentTokenKind::Keyword => HlTag::Keyword,
                DocCommentTokenKind::Parameter => HlTag::Parameter,
                DocCommentTokenKind::Property => HlTag::Property,
                DocCommentTokenKind::Type => HlTag::Type,
                DocCommentTokenKind::Reference => HlTag::Function,
            };
            highlights.push(HlRange {
                range,
                tag,
                modifiers: HlMod::new().with(HlMod::DOCUMENTATION),
            });
            start = range.end();
        }
        push_comment(highlights, TextRange::new(start, comment.range.end()));
    }
}

/// Splitting the surrounding prose avoids overlapping LSP semantic tokens.
fn push_comment(highlights: &mut Vec<HlRange>, range: TextRange) {
    if !range.is_empty() {
        highlights.push(HlRange { range, tag: HlTag::Comment, modifiers: HlMod::new() });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax_highlighting::{highlight, tests::create_db_with_file};

    /// Checks roles against source text, so Unicode offsets cannot pass accidentally.
    fn documented_tokens(code: &str) -> Vec<(String, HlTag)> {
        let (db, file) = create_db_with_file(code);
        let result = highlight(&db, file);
        for pair in result.highlights.windows(2) {
            assert!(pair[0].range.end() <= pair[1].range.start(), "overlap: {pair:?}");
        }
        result
            .highlights
            .iter()
            .filter(|token| token.modifiers.contains(HlMod::DOCUMENTATION))
            .map(|token| {
                (
                    code[usize::from(token.range.start())..usize::from(token.range.end())]
                        .to_string(),
                    token.tag,
                )
            })
            .collect()
    }

    /// A real method header includes fields, unions, annotations and references.
    #[test]
    fn russian_method_documentation() {
        let code = "\
// Вычисляет результат. См. ОбщийМодуль.ДругойМетод.
// Параметры:
//   Данные - Структура - входные данные
//     * Имя - Строка - имя
//   Результат - Число, Неопределено - значение
//
// Возвращаемое значение:
//   Массив из Строка - строки
// Пример:
//   Ответ = Вычислить(Данные);
&НаСервере
Функция Вычислить(Данные, Результат) Экспорт
    Возврат Неопределено;
КонецФункции
";
        let actual = documented_tokens(code);
        let expected = vec![
            ("См.", HlTag::Keyword),
            ("ОбщийМодуль.ДругойМетод", HlTag::Function),
            ("Параметры:", HlTag::Keyword),
            ("Данные", HlTag::Parameter),
            ("Структура", HlTag::Type),
            ("Имя", HlTag::Property),
            ("Строка", HlTag::Type),
            ("Результат", HlTag::Parameter),
            ("Число", HlTag::Type),
            ("Неопределено", HlTag::Type),
            ("Возвращаемое значение:", HlTag::Keyword),
            ("Массив", HlTag::Type),
            ("из", HlTag::Keyword),
            ("Строка", HlTag::Type),
            ("Пример:", HlTag::Keyword),
        ];
        assert_eq!(
            actual,
            expected.into_iter().map(|(text, tag)| (text.to_string(), tag)).collect::<Vec<_>>()
        );
    }

    /// English keywords, inline return types, tabs and CRLF share the same ranges.
    #[test]
    fn english_crlf_documentation() {
        let code = "\
// PARAMETERS:
// Value\t-\tString, Number - text
// Options - See Module.Options
// RETURNS: Array of String - values
// CALL OPTIONS:
//   Value - Number - this is example text
// Deprecated. See Module.Replacement
Async Function Test(Value, Options)
    Return Value;
EndFunction
"
        .replace('\n', "\r\n");
        let actual = documented_tokens(&code);
        let expected = vec![
            ("PARAMETERS:", HlTag::Keyword),
            ("Value", HlTag::Parameter),
            ("String", HlTag::Type),
            ("Number", HlTag::Type),
            ("Options", HlTag::Parameter),
            ("See", HlTag::Keyword),
            ("Module.Options", HlTag::Function),
            ("RETURNS:", HlTag::Keyword),
            ("Array", HlTag::Type),
            ("of", HlTag::Keyword),
            ("String", HlTag::Type),
            ("CALL OPTIONS:", HlTag::Keyword),
            ("Deprecated", HlTag::Keyword),
        ];
        assert_eq!(
            actual,
            expected.into_iter().map(|(text, tag)| (text.to_string(), tag)).collect::<Vec<_>>()
        );
    }

    /// Blank lines, code and inline comments must not become method documentation.
    #[test]
    fn unrelated_comments_and_strings_stay_unstructured() {
        for code in [
            "// Параметры:\n// Имя - Строка\n\nПроцедура Тест()\nКонецПроцедуры",
            "Процедура Тест()\n// Параметры:\n// Имя - Строка\nКонецПроцедуры",
            "Х = 1; // Параметры:\nПроцедура Тест()\nКонецПроцедуры",
            "// Параметры:\nХ = 1;\nПроцедура Тест()\nКонецПроцедуры",
            "Х = \"Параметры: Имя - Строка\";\nПроцедура Тест()\nКонецПроцедуры",
            "// Например: Имя - Строка\n// Ширина 5 см. Высота\nПроцедура Тест()\nКонецПроцедуры",
        ] {
            assert!(documented_tokens(code).is_empty(), "unexpected doc tokens in {code:?}");
        }
    }

    /// Type continuation lines and returned structure fields preserve their roles.
    #[test]
    fn continuation_types_and_return_fields() {
        let code = "\
// Параметры:
// Документ - ДокументСсылка.Первый,
//   ДокументСсылка.Второй - документ
//   - Неопределено - отсутствует
// Возвращаемое значение: Структура
//   * Имя - Строка - имя
//   * Данные - см. Модуль.Данные
Функция Тест(Документ)
КонецФункции";
        let actual = documented_tokens(code);
        assert!(actual.contains(&("ДокументСсылка.Второй".into(), HlTag::Type)));
        assert!(actual.contains(&("Неопределено".into(), HlTag::Type)));
        assert!(actual.contains(&("Имя".into(), HlTag::Property)));
        assert!(actual.contains(&("Данные".into(), HlTag::Property)));
        assert!(actual.contains(&("Модуль.Данные".into(), HlTag::Function)));
    }

    /// Repeated words and non-BMP text must retain distinct byte ranges.
    #[test]
    fn repeated_words_bom_and_unicode() {
        let code = "\u{feff}// 😀 описание\n// Параметры:\n// Строка - Строка - Строка\nПроцедура Тест(Строка)\nКонецПроцедуры";
        assert_eq!(
            documented_tokens(code),
            vec![
                ("Параметры:".into(), HlTag::Keyword),
                ("Строка".into(), HlTag::Parameter),
                ("Строка".into(), HlTag::Type),
            ]
        );
    }

    /// Removing the attachment boundary must immediately remove structural tokens.
    #[test]
    fn editing_documentation_attachment() {
        use ide_db::base_db::SourceDatabase;
        let code = "// Parameters:\n// Value - String\nProcedure Test(Value)\nEndProcedure";
        let (mut db, file) = create_db_with_file(code);
        assert!(highlight(&db, file)
            .highlights
            .iter()
            .any(|hl| hl.modifiers.contains(HlMod::DOCUMENTATION)));
        db.set_file_text(file, &code.replace("\nProcedure", "\n\nProcedure"));
        assert!(!highlight(&db, file)
            .highlights
            .iter()
            .any(|hl| hl.modifiers.contains(HlMod::DOCUMENTATION)));
    }
}
