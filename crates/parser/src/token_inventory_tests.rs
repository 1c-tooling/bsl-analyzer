//! Инвентарь видов лексем: каждый вид порождается входом и каждый значимый
//! вид читается правилом.
//!
//! Обе проверки живут на ОДНОЙ таблице свидетелей. Две копии таблицы в разных
//! крейтах разошлись бы молча, а канал SDBL наблюдаем только отсюда:
//! `sdbl_token_converter` объявлен приватным, и снаружи крейта его не видно.
//!
//! Рядом — ведомость написаний, выписанная из Главы 4 независимо от образцов
//! лексера: таблица свидетелей отвечает за вид в конструкции, ведомость — за
//! каждое написание вида.
//!
//! Provenance: `docs/legal/bsl-clean-room-slice-b1.md`.

use crate::event::Event;
use crate::syntax_kind::token_kind_to_syntax;
use lexer::{Token, TokenKind};
use syntax::{SyntaxKind, TextSize};

/// Канал, которым вид попадает в дерево.
///
/// `TokenKind` — общий алфавит двух лексеров. Вид, который BSL-лексер не
/// порождает, не обязан быть мёртвым: он может приходить преобразованием
/// лексем SDBL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Channel {
    /// Вид порождает BSL-лексер на `input`.
    Bsl,
    /// Вид приходит преобразованием лексем SDBL; BSL-лексер его не порождает.
    Sdbl,
}

struct Witness {
    kind: TokenKind,
    channel: Channel,
    input: &'static str,
    /// Текст BSL, на котором вид канала SDBL мог бы появиться, если бы
    /// BSL-лексер его порождал.
    ///
    /// Без такого зонда проверка непорождаемости зелена вхолостую: знаков
    /// `#`, `&`, `{`, `}`, `|` нет ни в одном законном входе таблицы, и
    /// перебор по ним не встретил бы нарушения ни при какой реализации.
    bsl_probe: Option<&'static str>,
}

const fn bsl(kind: TokenKind, input: &'static str) -> Witness {
    Witness { kind, channel: Channel::Bsl, input, bsl_probe: None }
}

const fn sdbl(kind: TokenKind, input: &'static str, bsl_probe: &'static str) -> Witness {
    Witness { kind, channel: Channel::Sdbl, input, bsl_probe: Some(bsl_probe) }
}

/// Свидетель на каждый вид, в порядке объявления перечисления.
///
/// Входы — законные конструкции языка, а не голые обрывки: тот же вход
/// отвечает и на вопрос «порождается ли вид», и на вопрос «читает ли его
/// правило», а на обрывке второй вопрос смысла не имеет.
const WITNESSES: &[Witness] = &[
    bsl(TokenKind::KwProcedure, "Процедура П() КонецПроцедуры"),
    bsl(TokenKind::KwEndProcedure, "Процедура П() КонецПроцедуры"),
    bsl(TokenKind::KwFunction, "Функция Ф() Возврат 1; КонецФункции"),
    bsl(TokenKind::KwEndFunction, "Функция Ф() Возврат 1; КонецФункции"),
    bsl(TokenKind::KwExport, "Процедура П() Экспорт КонецПроцедуры"),
    bsl(TokenKind::KwVal, "Процедура П(Знач А) КонецПроцедуры"),
    bsl(TokenKind::KwIf, "Процедура П() Если А Тогда КонецЕсли; КонецПроцедуры"),
    bsl(TokenKind::KwThen, "Процедура П() Если А Тогда КонецЕсли; КонецПроцедуры"),
    bsl(
        TokenKind::KwElsIf,
        "Процедура П() Если А Тогда ИначеЕсли Б Тогда КонецЕсли; КонецПроцедуры",
    ),
    bsl(TokenKind::KwElse, "Процедура П() Если А Тогда Иначе КонецЕсли; КонецПроцедуры"),
    bsl(TokenKind::KwEndIf, "Процедура П() Если А Тогда КонецЕсли; КонецПроцедуры"),
    bsl(TokenKind::KwFor, "Процедура П() Для С = 1 По 3 Цикл КонецЦикла; КонецПроцедуры"),
    bsl(TokenKind::KwEach, "Процедура П() Для Каждого Э Из К Цикл КонецЦикла; КонецПроцедуры"),
    bsl(TokenKind::KwIn, "Процедура П() Для Каждого Э Из К Цикл КонецЦикла; КонецПроцедуры"),
    bsl(TokenKind::KwTo, "Процедура П() Для С = 1 По 3 Цикл КонецЦикла; КонецПроцедуры"),
    bsl(TokenKind::KwWhile, "Процедура П() Пока А Цикл КонецЦикла; КонецПроцедуры"),
    bsl(TokenKind::KwDo, "Процедура П() Пока А Цикл КонецЦикла; КонецПроцедуры"),
    bsl(TokenKind::KwEndDo, "Процедура П() Пока А Цикл КонецЦикла; КонецПроцедуры"),
    bsl(TokenKind::KwReturn, "Функция Ф() Возврат 1; КонецФункции"),
    bsl(TokenKind::KwContinue, "Процедура П() Пока А Цикл Продолжить; КонецЦикла; КонецПроцедуры"),
    bsl(TokenKind::KwBreak, "Процедура П() Пока А Цикл Прервать; КонецЦикла; КонецПроцедуры"),
    bsl(TokenKind::KwGoto, "Процедура П() Перейти ~М; ~М: Возврат; КонецПроцедуры"),
    bsl(TokenKind::KwTry, "Процедура П() Попытка Исключение КонецПопытки; КонецПроцедуры"),
    bsl(TokenKind::KwExcept, "Процедура П() Попытка Исключение КонецПопытки; КонецПроцедуры"),
    bsl(TokenKind::KwEndTry, "Процедура П() Попытка Исключение КонецПопытки; КонецПроцедуры"),
    bsl(
        TokenKind::KwRaise,
        "Процедура П() Попытка Исключение ВызватьИсключение; КонецПопытки; КонецПроцедуры",
    ),
    bsl(TokenKind::KwVar, "Перем А;"),
    bsl(TokenKind::KwNew, "Процедура П() А = Новый Массив; КонецПроцедуры"),
    bsl(TokenKind::KwExecute, "Процедура П() Выполнить(\"А\"); КонецПроцедуры"),
    bsl(TokenKind::KwAddHandler, "Процедура П() ДобавитьОбработчик О.С, Обр; КонецПроцедуры"),
    bsl(TokenKind::KwRemoveHandler, "Процедура П() УдалитьОбработчик О.С, Обр; КонецПроцедуры"),
    bsl(TokenKind::KwAsync, "Асинх Функция Ф() Возврат Ждать Г(); КонецФункции"),
    bsl(TokenKind::KwAwait, "Асинх Функция Ф() Возврат Ждать Г(); КонецФункции"),
    bsl(TokenKind::KwAnd, "Процедура П() Если А И Б Тогда КонецЕсли; КонецПроцедуры"),
    bsl(TokenKind::KwOr, "Процедура П() Если А Или Б Тогда КонецЕсли; КонецПроцедуры"),
    bsl(TokenKind::KwNot, "Процедура П() Если Не А Тогда КонецЕсли; КонецПроцедуры"),
    bsl(TokenKind::KwTrue, "Процедура П() А = Истина; КонецПроцедуры"),
    bsl(TokenKind::KwFalse, "Процедура П() А = Ложь; КонецПроцедуры"),
    bsl(TokenKind::KwUndefined, "Процедура П() А = Неопределено; КонецПроцедуры"),
    bsl(TokenKind::KwNull, "Процедура П() А = NULL; КонецПроцедуры"),
    bsl(TokenKind::PreIf, "#Если Клиент Тогда\n#КонецЕсли"),
    bsl(TokenKind::PreElsIf, "#Если Клиент Тогда\n#ИначеЕсли Сервер Тогда\n#КонецЕсли"),
    bsl(TokenKind::PreElse, "#Если Клиент Тогда\n#Иначе\n#КонецЕсли"),
    bsl(TokenKind::PreEndIf, "#Если Клиент Тогда\n#КонецЕсли"),
    bsl(TokenKind::PreRegion, "#Область О\n#КонецОбласти"),
    bsl(TokenKind::PreEndRegion, "#Область О\n#КонецОбласти"),
    bsl(TokenKind::PreInsert, "#Вставка\nПроцедура П() КонецПроцедуры\n#КонецВставки"),
    bsl(TokenKind::PreEndInsert, "#Вставка\nПроцедура П() КонецПроцедуры\n#КонецВставки"),
    bsl(TokenKind::PreDelete, "#Удаление\nПроцедура П() КонецПроцедуры\n#КонецУдаления"),
    bsl(TokenKind::PreEndDelete, "#Удаление\nПроцедура П() КонецПроцедуры\n#КонецУдаления"),
    bsl(TokenKind::AnnAtClient, "&НаКлиенте\nПроцедура П() КонецПроцедуры"),
    bsl(TokenKind::AnnAtServer, "&НаСервере\nПроцедура П() КонецПроцедуры"),
    bsl(TokenKind::AnnAtServerNoContext, "&НаСервереБезКонтекста\nПроцедура П() КонецПроцедуры"),
    bsl(TokenKind::AnnAtClientAtServerNoContext, "&НаКлиентеНаСервереБезКонтекста\nПерем А;"),
    bsl(TokenKind::AnnAtClientAtServer, "&НаКлиентеНаСервере\nПерем А;"),
    bsl(TokenKind::AnnBefore, "&Перед(\"М\")\nПроцедура П() КонецПроцедуры"),
    bsl(TokenKind::AnnAfter, "&После(\"М\")\nПроцедура П() КонецПроцедуры"),
    bsl(TokenKind::AnnAround, "&Вместо(\"М\")\nПроцедура П() КонецПроцедуры"),
    bsl(
        TokenKind::AnnChangeAndValidate,
        "&ИзменениеИКонтроль(\"М\")\nПроцедура П() КонецПроцедуры",
    ),
    bsl(TokenKind::AnnCustom, "&МояАннотация\nПроцедура П() КонецПроцедуры"),
    bsl(TokenKind::Eq, "Процедура П() А = 1; КонецПроцедуры"),
    bsl(TokenKind::Neq, "Процедура П() Если А <> Б Тогда КонецЕсли; КонецПроцедуры"),
    bsl(TokenKind::Le, "Процедура П() Если А <= Б Тогда КонецЕсли; КонецПроцедуры"),
    bsl(TokenKind::Lt, "Процедура П() Если А < Б Тогда КонецЕсли; КонецПроцедуры"),
    bsl(TokenKind::Ge, "Процедура П() Если А >= Б Тогда КонецЕсли; КонецПроцедуры"),
    bsl(TokenKind::Gt, "Процедура П() Если А > Б Тогда КонецЕсли; КонецПроцедуры"),
    bsl(TokenKind::Plus, "Процедура П() А = Б + В; КонецПроцедуры"),
    bsl(TokenKind::Minus, "Процедура П() А = Б - В; КонецПроцедуры"),
    bsl(TokenKind::Star, "Процедура П() А = Б * В; КонецПроцедуры"),
    bsl(TokenKind::Slash, "Процедура П() А = Б / В; КонецПроцедуры"),
    bsl(TokenKind::Percent, "Процедура П() А = Б % В; КонецПроцедуры"),
    bsl(TokenKind::LParen, "Процедура П() Ф(А, Б); КонецПроцедуры"),
    bsl(TokenKind::RParen, "Процедура П() Ф(А, Б); КонецПроцедуры"),
    sdbl(TokenKind::LBrace, "ВЫБРАТЬ {Т.П} ИЗ Т", "Процедура П() А = {; КонецПроцедуры"),
    sdbl(TokenKind::RBrace, "ВЫБРАТЬ {Т.П} ИЗ Т", "Процедура П() А = }; КонецПроцедуры"),
    bsl(TokenKind::LBracket, "Процедура П() А = К[0]; КонецПроцедуры"),
    bsl(TokenKind::RBracket, "Процедура П() А = К[0]; КонецПроцедуры"),
    bsl(TokenKind::Dot, "Процедура П() А = О.С; КонецПроцедуры"),
    bsl(TokenKind::Comma, "Процедура П() Ф(А, Б); КонецПроцедуры"),
    bsl(TokenKind::Semicolon, "Процедура П() А = 1; КонецПроцедуры"),
    bsl(TokenKind::Colon, "Процедура П() Перейти ~М; ~М: Возврат; КонецПроцедуры"),
    bsl(TokenKind::Question, "Процедура П() А = ?(Б, 1, 2); КонецПроцедуры"),
    bsl(TokenKind::Tilde, "Процедура П() Перейти ~М; ~М: Возврат; КонецПроцедуры"),
    sdbl(TokenKind::Bar, "ВЫБРАТЬ |", "Процедура П() А = |; КонецПроцедуры"),
    sdbl(TokenKind::Hash, "ВЫБРАТЬ #", "#Неизвестная"),
    sdbl(TokenKind::Ampersand, "ВЫБРАТЬ * ИЗ Т ГДЕ Т.П = &Пар", "&1"),
    bsl(TokenKind::Float, "Процедура П() А = 1.5; КонецПроцедуры"),
    bsl(TokenKind::Decimal, "Процедура П() А = 1; КонецПроцедуры"),
    bsl(TokenKind::String, "Процедура П() А = \"с\"; КонецПроцедуры"),
    bsl(TokenKind::StringStart, "Процедура П() А = \"п\n|т\"; КонецПроцедуры"),
    bsl(TokenKind::StringTail, "Процедура П() А = \"п\n|т\"; КонецПроцедуры"),
    bsl(TokenKind::StringPart, "Процедура П() А = \"п\n|р\n|т\"; КонецПроцедуры"),
    bsl(TokenKind::Date, "Процедура П() А = '20240101'; КонецПроцедуры"),
    bsl(TokenKind::Ident, "Процедура П() А = Б; КонецПроцедуры"),
    bsl(TokenKind::Comment, "// комментарий\nПроцедура П() КонецПроцедуры"),
    bsl(TokenKind::Newline, "Процедура П()\nКонецПроцедуры"),
    bsl(TokenKind::Whitespace, "Процедура П() КонецПроцедуры"),
    bsl(TokenKind::Bom, "\u{FEFF}Процедура П() КонецПроцедуры"),
    bsl(TokenKind::Error, "Процедура П() А = 1;\u{2003} КонецПроцедуры"),
];

fn witness_of(kind: TokenKind) -> &'static Witness {
    WITNESSES
        .iter()
        .find(|w| w.kind == kind)
        .unwrap_or_else(|| panic!("{kind:?}: вида нет в таблице свидетелей"))
}

/// Таблица покрывает перечисление целиком и ровно по разу.
///
/// Без этого J1 и J2 молча сужаются до тех видов, о которых кто-то вспомнил:
/// свойство, проверенное на выборке, зелено и у инвентаря, разошедшегося на
/// не вошедшем в выборку виде.
#[test]
fn the_witness_table_covers_every_kind_exactly_once() {
    for kind in TokenKind::ALL {
        let hits = WITNESSES.iter().filter(|w| w.kind == *kind).count();
        assert_eq!(hits, 1, "{kind:?}: строк в таблице свидетелей {hits}, а должна быть одна");
    }
    assert_eq!(
        WITNESSES.len(),
        TokenKind::ALL.len(),
        "в таблице свидетелей есть строки, которым не отвечает ни один вид"
    );
    let mut sdbl: Vec<_> =
        WITNESSES.iter().filter(|w| w.channel == Channel::Sdbl).map(|w| w.kind).collect();
    sdbl.sort_by_key(|k| format!("{k:?}"));
    let mut expected = vec![
        TokenKind::LBrace,
        TokenKind::RBrace,
        TokenKind::Bar,
        TokenKind::Hash,
        TokenKind::Ampersand,
    ];
    expected.sort_by_key(|k| format!("{k:?}"));
    assert_eq!(sdbl, expected, "канал SDBL принадлежит ровно этим видам");
    assert_eq!(WITNESSES.iter().filter(|w| w.channel == Channel::Bsl).count(), 94);
    assert_eq!(TokenKind::ALL.iter().filter(|k| k.is_trivia()).count(), 4);
}

#[test]
fn original_inventory_has_one_attestation_per_variant() {
    let original: Vec<_> =
        include_str!("fixtures/token_inventory_original_kinds.txt").lines().collect();
    let document = include_str!("../../../docs/legal/bsl-clean-room-slice-b1.md");
    let rows: Vec<Vec<_>> = document
        .lines()
        .filter_map(|line| {
            let columns: Vec<_> = line
                .replace("\\|", "\u{1f}")
                .split('|')
                .map(|s| s.trim().replace('\u{1f}', "|"))
                .collect();
            (columns.len() == 9 && columns[1].parse::<usize>().is_ok()).then_some(columns)
        })
        .collect();
    let mut attested: Vec<_> = rows.iter().map(|r| r[2].trim_matches('`')).collect();
    let mut expected = original.clone();
    attested.sort();
    expected.sort();
    assert_eq!(expected.len(), 101);
    assert!(expected.windows(2).all(|pair| pair[0] != pair[1]));
    assert_eq!(attested, expected, "пропуск, лишний вид или дубликат вердикта");

    let mut live = Vec::new();
    let mut retired = Vec::new();
    for row in rows {
        let name = row[2].trim_matches('`').to_owned();
        assert!(!row[3].is_empty() && !row[4].is_empty() && !row[5].is_empty(), "{name}");
        if row[6] == "**удалён**" {
            retired.push(name.clone());
            assert_eq!(row[7], "—", "{name}: у удалённого вида нет канала");
        } else {
            live.push(name.to_owned());
            let w = WITNESSES.iter().find(|w| format!("{:?}", w.kind) == name).unwrap();
            match w.channel {
                Channel::Bsl => assert!(row[7].starts_with("BSL"), "{name}"),
                Channel::Sdbl => assert_eq!(row[7], "SDBL", "{name}"),
            }
            let status = match w.kind {
                TokenKind::LBrace
                | TokenKind::RBrace
                | TokenKind::Bar
                | TokenKind::Hash
                | TokenKind::Ampersand => "не BSL",
                TokenKind::KwAddHandler | TokenKind::KwRemoveHandler => {
                    "форма оператора, не резерв (§ 5)"
                }
                TokenKind::AnnAround => "форма + разрешение",
                TokenKind::AnnCustom => "разрешение IDE",
                TokenKind::Date => "форма + ограничение (§ 5)",
                TokenKind::Newline => "форма; отдельная лексема — устройство лексера",
                TokenKind::Whitespace => "разрешение lossless-дерева",
                TokenKind::Bom => "разрешение: артефакт кодировки",
                TokenKind::Error => "служебный итог `tokenize`",
                kind if OTHER_WORDS.iter().any(|w| w.kind == kind) => "форма, не резерв",
                _ => "форма",
            };
            assert_eq!(row[6], status, "{name}: вердикт должен соответствовать решению");
        }
    }
    retired.sort();
    assert_eq!(retired, ["Exclamation", "PreUse"]);
    live.sort();
    let mut kinds: Vec<_> = TokenKind::ALL.iter().map(|k| format!("{k:?}")).collect();
    kinds.sort();
    assert_eq!(live, kinds, "живые вердикты должны совпадать с ALL как множество");
}

/// J1 — каждый вид порождается хотя бы одним входом.
///
/// `TokenKind` — общий алфавит двух каналов, поэтому вход берётся из того
/// канала, который питает вид. Канал SDBL проверяется через публичный
/// `parse_sdbl`: это единственная проверка, что преобразование лексем SDBL
/// живо, и она падает, если конвертер или SDBL-лексер потеряют вид.
#[test]
fn every_kind_is_produced_by_some_input() {
    let mut unreachable = Vec::new();
    for kind in TokenKind::ALL {
        let w = witness_of(*kind);
        let produced = match w.channel {
            Channel::Bsl => lexer::tokenize(w.input).iter().any(|t| t.kind == *kind),
            Channel::Sdbl => {
                let want = token_kind_to_syntax(*kind);
                crate::parse_sdbl(w.input)
                    .syntax_node()
                    .descendants_with_tokens()
                    .filter_map(|e| e.into_token())
                    .any(|t| t.kind() == want)
            }
        };
        if !produced {
            unreachable.push(format!("{kind:?} ({:?}) на {:?}", w.channel, w.input));
        }
    }
    assert!(
        unreachable.is_empty(),
        "виды, которых не даёт ни один вход:\n  {}",
        unreachable.join("\n  ")
    );
}

/// Вид, объявленный каналом SDBL, BSL-лексер не порождает.
///
/// Это утверждение и есть решение «перестать порождать», записанное так,
/// чтобы оно могло упасть: вернули образец в BSL-лексер — тест красен.
/// Проверяется на именном зонде каждой такой строки И на всех входах канала
/// BSL, потому что зонд отвечает за нарушение в лоб, а входы — за то, что вид
/// не просочился в обычный код.
#[test]
fn a_kind_owned_by_the_sdbl_channel_is_never_lexed_from_bsl() {
    let sdbl_only: Vec<TokenKind> =
        WITNESSES.iter().filter(|w| w.channel == Channel::Sdbl).map(|w| w.kind).collect();

    let bsl_texts: Vec<&'static str> = WITNESSES
        .iter()
        .filter(|w| w.channel == Channel::Bsl)
        .map(|w| w.input)
        .chain(WITNESSES.iter().filter_map(|w| w.bsl_probe))
        .collect();

    let mut breaches = Vec::new();
    for text in bsl_texts {
        for token in lexer::tokenize(text) {
            if sdbl_only.contains(&token.kind) {
                breaches.push(format!("{:?} на {text:?}", token.kind));
            }
        }
    }

    assert!(
        breaches.is_empty(),
        "BSL-лексер порождает виды канала SDBL:\n  {}",
        breaches.join("\n  ")
    );
}

/// Лексемы входа и события его разбора — сырьё, из которого строится дерево.
fn bsl_events(input: &str) -> (Vec<Token>, Vec<Event>) {
    let tokens = lexer::tokenize(input);
    let mut p = crate::Parser::new(&tokens);
    crate::grammar::source_file(&mut p);
    let events = p.finish();
    (tokens, events)
}

/// Смещения значимых лексем, которые правило потребило: у лексемы есть своё
/// `Event::Token`, и ни один её предок в дереве из ТЕХ ЖЕ событий не `ERROR`.
///
/// Событие ищется сопоставлением, а не по дереву: i-е `Event::Token` сток
/// отдаёт i-й значимой лексеме, а хвост за последним событием досыпает
/// `take_the_tail` уже без события — под корень, где родитель не `ERROR`.
/// Предки проверяются все, а не ближайший: правило, не узнавшее оператор,
/// закрывает внешним `ERROR` уже собранный дочерний узел, и у лексемы внутри
/// него ближайший родитель законный.
fn consumed_offsets(tokens: &[Token], events: Vec<Event>) -> Vec<usize> {
    let with_event = events.iter().filter(|e| matches!(e, Event::Token { .. })).count();
    let evented: Vec<&Token> =
        tokens.iter().filter(|t| !t.kind.is_trivia()).take(with_event).collect();

    let root = crate::sink::Sink::new(tokens).finish(events).finish().syntax_node();
    evented
        .into_iter()
        .filter(|lexeme| {
            let at = TextSize::new(lexeme.offset as u32);
            root.token_at_offset(at).any(|t| {
                t.text_range().start() == at
                    && t.kind() == token_kind_to_syntax(lexeme.kind)
                    && t.parent_ancestors().all(|n| n.kind() != SyntaxKind::ERROR)
            })
        })
        .map(|lexeme| lexeme.offset)
        .collect()
}

/// J2 — значимый вид читается правилом, а не просто лежит в дереве.
///
/// Две очевидные формулировки дают гейт, который не может упасть. Покрытие
/// текста сходится всегда: `Sink::finish` сметает остаток входа в дерево.
/// Наличие родителя тоже: `Parser::emit_error` бампает нечитаемый токен в узел
/// `Error`. И даже «родитель не `ERROR`» пропускает хвост без события и
/// лексему дочернего узла под внешним `ERROR` — поэтому `consumed_offsets`.
///
/// Исключения названы по построению, а не по удобству:
/// - `TokenKind::Error` несёт текст, который не может назвать ни одно
///   правило, и `ERROR` — его единственный законный родитель;
/// - тривия по `TokenKind::is_trivia` привязывается `Sink`, а не правилом;
///   `T![…]` у тривиального вида ветви не имеет, то есть правило физически
///   не может её потребовать.
///
/// Область — виды канала BSL. Потребление видов канала SDBL держит слайс 12
/// SDBL, и эта граница здесь не двигается.
#[test]
fn every_significant_kind_is_consumed_by_a_rule() {
    let mut unread = Vec::new();
    for kind in TokenKind::ALL {
        if *kind == TokenKind::Error || kind.is_trivia() {
            continue;
        }
        let w = witness_of(*kind);
        if w.channel != Channel::Bsl {
            continue;
        }

        let (tokens, events) = bsl_events(w.input);
        let consumed = consumed_offsets(&tokens, events);
        if !tokens.iter().any(|t| t.kind == *kind && consumed.contains(&t.offset)) {
            unread.push(format!("{kind:?} на {:?}", w.input));
        }
    }
    assert!(
        unread.is_empty(),
        "виды без вхождения, потреблённого правилом вне узла ошибки:\n  {}",
        unread.join("\n  ")
    );
}

fn offset_of(input: &str, needle: &str) -> usize {
    input.find(needle).unwrap_or_else(|| panic!("{needle:?} нет во входе {input:?}"))
}

/// Проверка потребления различает три способа лежать в дереве не будучи
/// прочитанным — и не путает их с законным чтением.
///
/// Каждый отрицательный вход сохраняет текст и законного ближайшего
/// родителя там, где это возможно: именно на таких входах прежняя проверка
/// одного родителя была зелена.
#[test]
fn consumption_is_not_mistaken_for_recovery_or_tail() {
    let legal = "Процедура П() Если А Тогда КонецЕсли; КонецПроцедуры";
    let (tokens, events) = bsl_events(legal);
    assert!(
        consumed_offsets(&tokens, events).contains(&offset_of(legal, "Тогда")),
        "законный `Тогда` в условном операторе не засчитан"
    );

    let bare = "Тогда";
    let (tokens, events) = bsl_events(bare);
    assert!(
        !consumed_offsets(&tokens, events).contains(&0),
        "голый `Тогда` засчитан, хотя лежит только под узлом ошибки"
    );

    // `А;` не вызов и не присваивание: `assignment_or_call` закрывает внешним
    // `ERROR` уже собранный узел имени.
    let nested = "Процедура П() А; КонецПроцедуры";
    let (tokens, events) = bsl_events(nested);
    let at = offset_of(nested, "А;");
    let root = crate::parse(nested).syntax_node();
    let token = root
        .token_at_offset(TextSize::new(at as u32))
        .find(|t| t.kind() == SyntaxKind::IDENT)
        .expect("имя `А` лежит в дереве");
    assert_ne!(
        token.parent().map(|p| p.kind()),
        Some(SyntaxKind::ERROR),
        "вход перестал быть контролем: ближайший родитель имени уже `ERROR`"
    );
    assert!(
        token.parent_ancestors().any(|n| n.kind() == SyntaxKind::ERROR),
        "вход перестал быть контролем: над именем нет узла ошибки"
    );
    assert!(
        !consumed_offsets(&tokens, events).contains(&at),
        "лексема дочернего узла под внешним `ERROR` засчитана"
    );

    // Событие последней значимой лексемы снято: её досыплет хвост стока под
    // корень, и текст дерева останется прежним.
    let tail = "Процедура П() КонецПроцедуры";
    let (tokens, mut events) = bsl_events(tail);
    let last = events
        .iter()
        .rposition(|e| matches!(e, Event::Token { kind: TokenKind::KwEndProcedure }))
        .expect("у `КонецПроцедуры` есть событие");
    events.remove(last);
    let root = crate::sink::Sink::new(&tokens).finish(events.clone()).finish().syntax_node();
    assert_eq!(root.text().to_string(), tail, "хвост стока потерял текст");
    let at = TextSize::new(offset_of(tail, "КонецПроцедуры") as u32);
    let token = root.token_at_offset(at).find(|t| t.text_range().start() == at).unwrap();
    assert_eq!(token.parent().unwrap().kind(), SyntaxKind::SOURCE_FILE);
    assert!(
        !consumed_offsets(&tokens, events).contains(&offset_of(tail, "КонецПроцедуры")),
        "лексема из хвоста стока без `Event::Token` засчитана"
    );
}

#[test]
fn consumption_tracks_occurrences_through_trivia_and_forward_parents() {
    let input = "\u{feff}// начало\nПроцедура П()\nА = Б + В; А; КонецПроцедуры\n";
    let (tokens, events) = bsl_events(input);
    assert!(events.iter().any(|e| matches!(e, Event::Start { forward_parent: Some(_), .. })));
    let offsets = consumed_offsets(&tokens, events);
    let good = offset_of(input, "А =");
    let recovered = offset_of(input, "А;");
    assert!(tokens.iter().any(|t| t.kind == TokenKind::Ident && t.offset == good));
    assert!(tokens.iter().any(|t| t.kind == TokenKind::Ident && t.offset == recovered));
    assert!(offsets.contains(&good), "законное вхождение того же вида должно быть засчитано");
    assert!(!offsets.contains(&recovered), "вхождение под ERROR не заменяется законным соседом");
    for text in ["Б", "+", "В"] {
        assert!(offsets.contains(&offset_of(input, text)), "{text}: дочерний узел выражения");
    }
    assert!(tokens.iter().filter(|t| t.kind.is_trivia()).all(|t| !offsets.contains(&t.offset)));
}

/// Слово источника: пара написаний одной строки таблицы или раздела Главы 4.
///
/// Ведомость выписана из источника, а не выведена из образцов лексера:
/// написание, которого лексер не знает, видно только так. Таблице
/// свидетелей она не замена — та проверяет потребление вида в конструкции,
/// эта все написания вида.
struct Word {
    section: &'static str,
    kind: TokenKind,
    ru: &'static str,
    en: &'static str,
}

const fn word(section: &'static str, kind: TokenKind, ru: &'static str, en: &'static str) -> Word {
    Word { section, kind, ru, en }
}

/// Таблица 4.2.4.6 — тридцать пар зарезервированных слов.
const RESERVED_WORDS: &[Word] = &[
    word("4.2.4.6", TokenKind::KwIf, "Если", "If"),
    word("4.2.4.6", TokenKind::KwThen, "Тогда", "Then"),
    word("4.2.4.6", TokenKind::KwElsIf, "ИначеЕсли", "ElsIf"),
    word("4.2.4.6", TokenKind::KwElse, "Иначе", "Else"),
    word("4.2.4.6", TokenKind::KwEndIf, "КонецЕсли", "EndIf"),
    word("4.2.4.6", TokenKind::KwFor, "Для", "For"),
    word("4.2.4.6", TokenKind::KwEach, "Каждого", "Each"),
    word("4.2.4.6", TokenKind::KwIn, "Из", "In"),
    word("4.2.4.6", TokenKind::KwTo, "По", "To"),
    word("4.2.4.6", TokenKind::KwWhile, "Пока", "While"),
    word("4.2.4.6", TokenKind::KwDo, "Цикл", "Do"),
    word("4.2.4.6", TokenKind::KwEndDo, "КонецЦикла", "EndDo"),
    word("4.2.4.6", TokenKind::KwProcedure, "Процедура", "Procedure"),
    word("4.2.4.6", TokenKind::KwEndProcedure, "КонецПроцедуры", "EndProcedure"),
    word("4.2.4.6", TokenKind::KwFunction, "Функция", "Function"),
    word("4.2.4.6", TokenKind::KwEndFunction, "КонецФункции", "EndFunction"),
    word("4.2.4.6", TokenKind::KwVar, "Перем", "Var"),
    word("4.2.4.6", TokenKind::KwGoto, "Перейти", "Goto"),
    word("4.2.4.6", TokenKind::KwReturn, "Возврат", "Return"),
    word("4.2.4.6", TokenKind::KwContinue, "Продолжить", "Continue"),
    word("4.2.4.6", TokenKind::KwBreak, "Прервать", "Break"),
    word("4.2.4.6", TokenKind::KwAnd, "И", "And"),
    word("4.2.4.6", TokenKind::KwOr, "Или", "Or"),
    word("4.2.4.6", TokenKind::KwNot, "Не", "Not"),
    word("4.2.4.6", TokenKind::KwTry, "Попытка", "Try"),
    word("4.2.4.6", TokenKind::KwExcept, "Исключение", "Except"),
    word("4.2.4.6", TokenKind::KwEndTry, "КонецПопытки", "EndTry"),
    word("4.2.4.6", TokenKind::KwRaise, "ВызватьИсключение", "Raise"),
    word("4.2.4.6", TokenKind::KwNew, "Новый", "New"),
    word("4.2.4.6", TokenKind::KwExecute, "Выполнить", "Execute"),
];

/// Слова вне таблицы 4.2.4.6, каждое из раздела своей конструкции.
///
/// У NULL написание одно: 4.3.2 даёт его без пары.
const OTHER_WORDS: &[Word] = &[
    word("4.6.1", TokenKind::KwExport, "Экспорт", "Export"),
    word("4.7.4.2", TokenKind::KwVal, "Знач", "Val"),
    word("4.6.3", TokenKind::KwAsync, "Асинх", "Async"),
    word("4.6.9", TokenKind::KwAwait, "Ждать", "Await"),
    word("4.3.3", TokenKind::KwTrue, "Истина", "True"),
    word("4.3.3", TokenKind::KwFalse, "Ложь", "False"),
    word("4.3.5", TokenKind::KwUndefined, "Неопределено", "Undefined"),
    word("4.3.2", TokenKind::KwNull, "NULL", "NULL"),
    word("4.6.11.1", TokenKind::KwAddHandler, "ДобавитьОбработчик", "AddHandler"),
    word("4.6.11.2", TokenKind::KwRemoveHandler, "УдалитьОбработчик", "RemoveHandler"),
];

/// Инструкции 4.8.1.2, директивы 4.8.1.3 и аннотации 4.8.2 — со своим зачином.
const PREFIXED_WORDS: &[Word] = &[
    word("4.8.1.2", TokenKind::PreIf, "#Если", "#If"),
    word("4.8.1.2", TokenKind::PreElsIf, "#ИначеЕсли", "#ElsIf"),
    word("4.8.1.2", TokenKind::PreElse, "#Иначе", "#Else"),
    word("4.8.1.2", TokenKind::PreEndIf, "#КонецЕсли", "#EndIf"),
    word("4.8.1.2", TokenKind::PreRegion, "#Область", "#Region"),
    word("4.8.1.2", TokenKind::PreEndRegion, "#КонецОбласти", "#EndRegion"),
    word("4.8.1.2", TokenKind::PreInsert, "#Вставка", "#Insert"),
    word("4.8.1.2", TokenKind::PreEndInsert, "#КонецВставки", "#EndInsert"),
    word("4.8.1.2", TokenKind::PreDelete, "#Удаление", "#Delete"),
    word("4.8.1.2", TokenKind::PreEndDelete, "#КонецУдаления", "#EndDelete"),
    word("4.8.1.3", TokenKind::AnnAtClient, "&НаКлиенте", "&AtClient"),
    word("4.8.1.3", TokenKind::AnnAtServer, "&НаСервере", "&AtServer"),
    word(
        "4.8.1.3",
        TokenKind::AnnAtServerNoContext,
        "&НаСервереБезКонтекста",
        "&AtServerNoContext",
    ),
    word(
        "4.8.1.3",
        TokenKind::AnnAtClientAtServerNoContext,
        "&НаКлиентеНаСервереБезКонтекста",
        "&AtClientAtServerNoContext",
    ),
    word("4.8.1.3", TokenKind::AnnAtClientAtServer, "&НаКлиентеНаСервере", "&AtClientAtServer"),
    word("4.8.2", TokenKind::AnnBefore, "&Перед", "&Before"),
    word("4.8.2", TokenKind::AnnAfter, "&После", "&After"),
    word("4.8.2", TokenKind::AnnAround, "&Вместо", "&Around"),
    word("4.8.2", TokenKind::AnnChangeAndValidate, "&ИзменениеИКонтроль", "&ChangeAndValidate"),
];

/// Символы препроцессора 4.8.1.2 — тринадцать пар.
///
/// Их значение — дело следующего слайса; здесь только лексический итог: без
/// `#`/`&` каждое написание — имя, в том числе `НаКлиенте` рядом с
/// директивой `&НаКлиенте`.
const PREPROCESSOR_SYMBOLS: &[(&str, &str)] = &[
    ("Сервер", "Server"),
    ("НаСервере", "AtServer"),
    ("Клиент", "Client"),
    ("НаКлиенте", "AtClient"),
    ("ТонкийКлиент", "ThinClient"),
    ("МобильныйКлиент", "MobileClient"),
    ("ВебКлиент", "WebClient"),
    ("ВнешнееСоединение", "ExternalConnection"),
    ("ТолстыйКлиентУправляемоеПриложение", "ThickClientManagedApplication"),
    ("ТолстыйКлиентОбычноеПриложение", "ThickClientOrdinaryApplication"),
    ("МобильноеПриложениеКлиент", "MobileAppClient"),
    ("МобильноеПриложениеСервер", "MobileAppServer"),
    ("МобильныйАвтономныйСервер", "MobileStandaloneServer"),
];

/// Форма источника, которая лексемой не исчерпывается или задана знаком, и
/// её ожидаемая последовательность видов, включая тривию.
///
/// Дробление составной формы на лексемы — решение lossless-разбора, а не
/// источника; источник задаёт саму форму.
struct Form {
    section: &'static str,
    text: &'static str,
    kinds: &'static [TokenKind],
}

const fn form(section: &'static str, text: &'static str, kinds: &'static [TokenKind]) -> Form {
    Form { section, text, kinds }
}

const FORMS: &[Form] = &[
    form("4.2.5", "=", &[TokenKind::Eq]),
    form("4.2.5", "<>", &[TokenKind::Neq]),
    form("4.2.5", "<=", &[TokenKind::Le]),
    form("4.2.5", "<", &[TokenKind::Lt]),
    form("4.2.5", ">=", &[TokenKind::Ge]),
    form("4.2.5", ">", &[TokenKind::Gt]),
    form("4.2.5", "+", &[TokenKind::Plus]),
    form("4.2.5", "-", &[TokenKind::Minus]),
    form("4.2.5", "*", &[TokenKind::Star]),
    form("4.2.5", "/", &[TokenKind::Slash]),
    form("4.2.5", "%", &[TokenKind::Percent]),
    form("4.2.5", "(", &[TokenKind::LParen]),
    form("4.2.5", ")", &[TokenKind::RParen]),
    form("4.2.5", "[", &[TokenKind::LBracket]),
    form("4.2.5", "]", &[TokenKind::RBracket]),
    form("4.2.5", ".", &[TokenKind::Dot]),
    form("4.2.5", ",", &[TokenKind::Comma]),
    form("4.2.5", ";", &[TokenKind::Semicolon]),
    form("4.2.5", ":", &[TokenKind::Colon]),
    form("4.2.5", "~", &[TokenKind::Tilde]),
    form("4.2.4.2", "~М:", &[TokenKind::Tilde, TokenKind::Ident, TokenKind::Colon]),
    form("4.2.4.2", "\n", &[TokenKind::Newline]),
    form(
        "4.6.5.2",
        "?(А, 1, 2)",
        &[
            TokenKind::Question,
            TokenKind::LParen,
            TokenKind::Ident,
            TokenKind::Comma,
            TokenKind::Whitespace,
            TokenKind::Decimal,
            TokenKind::Comma,
            TokenKind::Whitespace,
            TokenKind::Decimal,
            TokenKind::RParen,
        ],
    ),
    form("4.2.4.1", "// комментарий", &[TokenKind::Comment]),
    form("4.3.6", "\"строка\"", &[TokenKind::String]),
    form("4.3.6", "\"а\"\"б\"", &[TokenKind::String]),
    form(
        "4.3.6",
        "\"п\n|т\"",
        &[TokenKind::StringStart, TokenKind::Newline, TokenKind::StringTail],
    ),
    form(
        "4.3.6",
        "\"п\n|р\n|т\"",
        &[
            TokenKind::StringStart,
            TokenKind::Newline,
            TokenKind::StringPart,
            TokenKind::Newline,
            TokenKind::StringTail,
        ],
    ),
    form("4.3.4", "'20240101'", &[TokenKind::Date]),
    form("4.3.4", "'2017\\03\\23 10~45~25'", &[TokenKind::Date]),
    form("4.3.8", "1", &[TokenKind::Decimal]),
    form("4.3.8", "1.5", &[TokenKind::Float]),
    form("4.3.8; 4.5.4", "-1", &[TokenKind::Minus, TokenKind::Decimal]),
    form("4.2.4.3", "_Имя1", &[TokenKind::Ident]),
    form("4.2.4.3", "Name_2", &[TokenKind::Ident]),
];

/// Виды канала BSL, у которых написания в Главе 4 нет, с основанием.
///
/// Перечень закрыт: вид, не попавший сюда и не получивший ни одного
/// написания из ведомости, — пропуск обратного хода.
const NOT_FROM_SOURCE: &[(TokenKind, &str)] = &[
    (TokenKind::AnnCustom, "4.8.2: пользовательских аннотаций система не поддерживает"),
    (TokenKind::Whitespace, "класс пробелов Главой 4 не задан"),
    (TokenKind::Bom, "артефакт кодировки файла"),
    (TokenKind::Error, "нераспознанный текст"),
];

/// Написания, которые лексер принимает сверх источника, — с основанием.
///
/// Держатся отдельно от ведомости, чтобы разрешение не выдавалось за форму
/// языка.
const ALLOWANCES: &[(&str, TokenKind, &str)] = &[
    ("&Instead", TokenKind::AnnAround, "совместимость с предшественником"),
    ("&МояАннотация", TokenKind::AnnCustom, "восстановление заголовка метода"),
];

fn all_words() -> impl Iterator<Item = &'static Word> {
    RESERVED_WORDS.iter().chain(OTHER_WORDS).chain(PREFIXED_WORDS)
}

/// Написание в регистрах, которые 4.2.4.5 объявляет равными.
fn case_variants(spelling: &str) -> [String; 4] {
    let mixed = spelling
        .chars()
        .enumerate()
        .map(|(i, c)| if i % 2 == 0 { c.to_uppercase().next() } else { c.to_lowercase().next() })
        .map(|c| c.expect("у знака есть регистровая пара или он сам"))
        .collect();
    [spelling.to_owned(), spelling.to_lowercase(), spelling.to_uppercase(), mixed]
}

fn lex_kinds(text: &str) -> Vec<TokenKind> {
    lexer::tokenize(text).iter().map(|t| t.kind).collect()
}

/// Одна лексема ровно во весь текст: вид, текст и обе границы.
fn assert_single(text: &str, kind: TokenKind, why: &str) {
    let tokens = lexer::tokenize(text);
    assert!(
        tokens.len() == 1
            && tokens[0].kind == kind
            && tokens[0].text == text
            && tokens[0].offset == 0,
        "{why}: {text:?} должно быть одной лексемой {kind:?}, а лексер дал {:?}",
        tokens.iter().map(|t| (t.kind, t.text.as_str())).collect::<Vec<_>>()
    );
}

/// Ведомость сама полна: тридцать пар таблицы, тринадцать символов,
/// ни один вид и ни одно написание не записаны дважды.
///
/// Без этого выпавшая строка ведомости молча сужает проверку написаний.
#[test]
fn the_source_ledger_is_complete_and_has_no_duplicates() {
    assert_eq!(RESERVED_WORDS.len(), 30, "в таблице 4.2.4.6 тридцать пар");
    assert!(RESERVED_WORDS.iter().all(|w| w.section == "4.2.4.6" && w.kind.is_keyword()));
    assert_eq!(PREPROCESSOR_SYMBOLS.len(), 13, "в перечне символов 4.8.1.2 тринадцать пар");

    let mut kinds = Vec::new();
    let mut spellings = Vec::new();
    for w in all_words() {
        assert!(!kinds.contains(&w.kind), "{:?} записан в ведомости дважды", w.kind);
        kinds.push(w.kind);
        for s in if w.ru == w.en { vec![w.ru] } else { vec![w.ru, w.en] } {
            let s = s.to_lowercase();
            assert!(!spellings.contains(&s), "написание {s:?} записано дважды");
            spellings.push(s);
        }
    }
    for (ru, en) in PREPROCESSOR_SYMBOLS {
        for s in [ru.to_lowercase(), en.to_lowercase()] {
            assert!(!spellings.contains(&s), "написание {s:?} записано дважды");
            spellings.push(s);
        }
    }
    for f in FORMS {
        assert!(!spellings.contains(&f.text.to_owned()), "форма {:?} записана дважды", f.text);
        spellings.push(f.text.to_owned());
    }
}

/// Обратный ход: каждый вид канала BSL получает написание из Главы 4 либо
/// стоит в закрытом перечне видов без источника.
#[test]
fn every_bsl_kind_has_a_source_spelling_or_a_stated_reason() {
    let from_source: Vec<TokenKind> = all_words()
        .map(|w| w.kind)
        .chain(std::iter::once(TokenKind::Ident))
        .chain(FORMS.iter().flat_map(|f| f.kinds.iter().copied()))
        .collect();

    let mut missing = Vec::new();
    for w in WITNESSES.iter().filter(|w| w.channel == Channel::Bsl) {
        let stated = NOT_FROM_SOURCE.iter().any(|(k, _)| *k == w.kind);
        if !stated && !from_source.contains(&w.kind) {
            missing.push(format!("{:?}", w.kind));
        }
        if stated && w.kind != TokenKind::Whitespace && from_source.contains(&w.kind) {
            missing.push(format!("{:?}: стоит в перечне без источника, но написание есть", w.kind));
        }
    }
    assert!(missing.is_empty(), "виды без написания и без основания:\n  {}", missing.join("\n  "));
}

/// I2 — каждое написание источника даёт записанный лексический итог в любом
/// регистре, а слово, продолженное знаком имени, становится именем.
#[test]
fn every_source_spelling_lexes_to_its_recorded_kind() {
    for w in all_words() {
        for spelling in [w.ru, w.en] {
            for variant in case_variants(spelling) {
                assert_single(&variant, w.kind, w.section);
            }
            if !spelling.starts_with(['#', '&']) {
                for longer in
                    [format!("{spelling}А"), format!("{spelling}1"), format!("{spelling}_")]
                {
                    assert_single(&longer, TokenKind::Ident, "продолжение имени");
                }
            }
        }
    }

    for (ru, en) in PREPROCESSOR_SYMBOLS {
        for spelling in [ru, en] {
            for variant in case_variants(spelling) {
                assert_single(&variant, TokenKind::Ident, "символ препроцессора 4.8.1.2");
            }
        }
    }

    for f in FORMS {
        let tokens = lexer::tokenize(f.text);
        let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.kind).collect();
        assert_eq!(kinds, f.kinds, "{} {:?}", f.section, f.text);
        let mut at = 0;
        for t in &tokens {
            assert_eq!(t.offset, at, "{:?}: разрыв между лексемами", f.text);
            assert_eq!(t.text, f.text[at..at + t.text.len()], "текст составной формы");
            at += t.text.len();
        }
        assert_eq!(at, f.text.len(), "{:?}: лексемы не покрыли форму", f.text);
    }

    for (text, kind, why) in ALLOWANCES {
        assert_single(text, *kind, why);
    }
}

/// I3 — знак без формы BSL остаётся `Error`, а не именованным видом, при
/// этом целая инструкция или директива по-прежнему узнаётся целиком.
#[test]
fn bare_signs_without_a_bsl_form_stay_errors() {
    for (text, kinds) in [
        ("#", vec![TokenKind::Error]),
        ("&", vec![TokenKind::Error]),
        ("!", vec![TokenKind::Error]),
        ("{", vec![TokenKind::Error]),
        ("}", vec![TokenKind::Error]),
        ("#Неизвестная", vec![TokenKind::Error, TokenKind::Ident]),
        ("&1", vec![TokenKind::Error, TokenKind::Decimal]),
        ("|", vec![TokenKind::StringPart]),
        ("#Если", vec![TokenKind::PreIf]),
        ("&AtClient", vec![TokenKind::AnnAtClient]),
        ("AtClient", vec![TokenKind::Ident]),
    ] {
        assert_eq!(lex_kinds(text), kinds, "{text:?}");
    }
}

/// I4 — операторы 4.6.11 на обоих языках дают свой вид и свой узел без
/// ошибок разбора.
#[test]
fn handler_statements_parse_in_both_languages() {
    for (text, kind, node) in [
        (
            "Процедура П() ДобавитьОбработчик О.С, Обр; КонецПроцедуры",
            TokenKind::KwAddHandler,
            SyntaxKind::ADD_HANDLER_STMT,
        ),
        (
            "Procedure P() AddHandler O.E, H; EndProcedure",
            TokenKind::KwAddHandler,
            SyntaxKind::ADD_HANDLER_STMT,
        ),
        (
            "Процедура П() УдалитьОбработчик О.С, Обр; КонецПроцедуры",
            TokenKind::KwRemoveHandler,
            SyntaxKind::REMOVE_HANDLER_STMT,
        ),
        (
            "Procedure P() RemoveHandler O.E, H; EndProcedure",
            TokenKind::KwRemoveHandler,
            SyntaxKind::REMOVE_HANDLER_STMT,
        ),
    ] {
        let parse = crate::parse(text);
        assert!(parse.errors().is_empty(), "{text:?}: {:?}", parse.errors());
        let root = parse.syntax_node();
        let stmt = root
            .descendants()
            .find(|n| n.kind() == node)
            .unwrap_or_else(|| panic!("{text:?}: нет узла {node:?}"));
        let first = stmt.first_token().expect("у оператора есть лексемы");
        assert_eq!(first.kind(), token_kind_to_syntax(kind), "{text:?}");

        let (tokens, events) = bsl_events(text);
        let at = tokens.iter().find(|t| t.kind == kind).expect("вид есть во входе").offset;
        assert!(consumed_offsets(&tokens, events).contains(&at), "{text:?}");
    }
}

const HANDLER_SPELLINGS: [&str; 4] =
    ["добавитьобработчик", "addhandler", "удалитьобработчик", "removehandler"];
const BARE_SIGNS: [&str; 5] = ["#", "&", "!", "{", "}"];

/// Счётчики корпуса по лексемам, а не по тексту файла: знак внутри строки
/// или комментария сюда не попадает.
#[derive(Default)]
struct CorpusCount {
    files: usize,
    unreadable: usize,
    /// Написания обработчиков 4.6.11 независимо от вида — и оператор, и имя.
    handlers: [usize; 4],
    /// Голые знаки любого вида, в том числе `Error`.
    signs: [usize; 5],
    signs_as_error: [usize; 5],
    question: usize,
    bar_as_string_part: usize,
}

impl CorpusCount {
    fn add_text(&mut self, text: &str) {
        self.files += 1;
        for t in lexer::tokenize(text) {
            let lower = t.text.to_lowercase();
            if let Some(i) = HANDLER_SPELLINGS.iter().position(|s| *s == lower) {
                self.handlers[i] += 1;
            }
            if let Some(i) = BARE_SIGNS.iter().position(|s| *s == t.text) {
                self.signs[i] += 1;
                if t.kind == TokenKind::Error {
                    self.signs_as_error[i] += 1;
                }
            }
            match (t.kind, t.text.as_str()) {
                (TokenKind::Question, _) => self.question += 1,
                (TokenKind::StringPart, "|") => self.bar_as_string_part += 1,
                _ => {}
            }
        }
    }

    fn add_file(&mut self, path: &std::path::Path) {
        match std::fs::read(path).map(String::from_utf8) {
            Ok(Ok(text)) => self.add_text(&text),
            _ => self.unreadable += 1,
        }
    }

    fn report(&self, name: &str) -> String {
        format!(
            "{name}: файлов {}, нечитаемых {}; обработчики {HANDLER_SPELLINGS:?} = {:?}; \
             знаки {BARE_SIGNS:?} = {:?} (из них Error {:?}); Question {}",
            self.files,
            self.unreadable,
            self.handlers,
            self.signs,
            self.signs_as_error,
            self.question
        )
    }
}

fn bsl_files_under(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let entries = std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    for entry in entries {
        let path = entry.expect("элемент каталога читается").path();
        let file_type = std::fs::symlink_metadata(&path).expect("метаданные читаются").file_type();
        if file_type.is_dir() {
            bsl_files_under(&path, out);
        } else if file_type.is_file() && path.extension().is_some_and(|e| e == "bsl") {
            out.push(path);
        }
    }
}

/// Счётчик корпуса видит каждое написание ровно там, где оно лексема.
///
/// Нули корпуса без этого контроля не читаются: счётчик, не способный
/// насчитать единицу, дал бы те же нули.
#[test]
fn the_corpus_counter_sees_each_spelling_once() {
    let mut count = CorpusCount::default();
    count.add_text(
        "ДобавитьОбработчик О.С, Обр; RemoveHandler О.С, Обр;\n\
         AddHandler О.С, Обр; УдалитьОбработчик О.С, Обр;\n\
         // ДобавитьОбработчик # & ! { }\n\
         А = \"AddHandler # & ! { }\";\n\
         #\n&\n!\n{\n}\n?(А,1,2)\n|",
    );
    assert_eq!(count.handlers, [1; 4], "написания обработчиков");
    assert_eq!(count.signs, [1; 5], "голые знаки");
    assert_eq!(count.signs_as_error, [1; 5], "голые знаки как Error");
    assert_eq!(count.question, 1, "Question");
    assert_eq!(count.bar_as_string_part, 1, "одиночный `|` как StringPart");
}

/// Отчёт по выбранному корпусу: обработчики 4.6.11 и голые знаки.
///
/// Корни конфигураций лежат вне репозитория и передаются окружением —
/// `BSL_TOKEN_CORPUS`, через `:`; репозиторные фикстуры — ровно
/// `git ls-files '*.bsl'`. Числа — наблюдение, а не константы приёмки:
/// отчёт требует три разных непустых корня и отсутствие сбоев чтения.
#[test]
#[ignore = "корпус вне репозитория; корни — в BSL_TOKEN_CORPUS"]
fn corpus_report() {
    let roots = std::env::var("BSL_TOKEN_CORPUS").expect("BSL_TOKEN_CORPUS не задан");
    let mut reports = Vec::new();
    let mut total = CorpusCount::default();

    let roots: Vec<_> = roots.split(':').filter(|r| !r.is_empty()).collect();
    assert_eq!(roots.len(), 3, "нужны все три корня выбранного корпуса");
    let distinct: std::collections::HashSet<_> = roots
        .iter()
        .map(|r| std::fs::canonicalize(r).expect("корень корпуса существует"))
        .collect();
    assert_eq!(distinct.len(), roots.len(), "корни корпуса не должны повторяться");
    for root in roots {
        let mut files = Vec::new();
        bsl_files_under(std::path::Path::new(root), &mut files);
        let mut count = CorpusCount::default();
        for file in &files {
            count.add_file(file);
        }
        assert!(count.files > 0, "{root}: ни одного читаемого .bsl");
        reports.push((root.to_owned(), count));
    }

    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let listed = std::process::Command::new("git")
        // `-z` снимает квотирование не-ASCII путей: при `core.quotePath`
        // по умолчанию кириллические пути пришли бы в кавычках с эскейпами.
        .args(["ls-files", "-z", "*.bsl"])
        .current_dir(&workspace)
        .output()
        .expect("git ls-files запускается");
    assert!(listed.status.success(), "git ls-files: {:?}", listed.status);
    let mut fixtures = CorpusCount::default();
    for path in String::from_utf8(listed.stdout).expect("пути в UTF-8").split_terminator('\0')
    {
        fixtures.add_file(&workspace.join(path));
    }
    reports.push(("git ls-files '*.bsl'".to_owned(), fixtures));

    for (name, count) in &reports {
        total.files += count.files;
        total.unreadable += count.unreadable;
        for i in 0..4 {
            total.handlers[i] += count.handlers[i];
        }
        for i in 0..5 {
            total.signs[i] += count.signs[i];
            total.signs_as_error[i] += count.signs_as_error[i];
        }
        total.question += count.question;
        println!("{}", count.report(name));
    }
    println!("{}", total.report("итого"));
    assert_eq!(total.unreadable, 0, "в корпусе есть нечитаемые файлы");
}

fn structural_dump(input: &str, sdbl: bool) -> String {
    fn node(out: &mut String, n: syntax::SyntaxNode) {
        use std::fmt::Write;
        writeln!(out, "N {:?} {:?} {{", n.kind(), n.text_range()).unwrap();
        for child in n.children_with_tokens() {
            if let Some(n) = child.as_node() {
                node(out, n.clone());
            } else if let Some(t) = child.as_token() {
                writeln!(out, "T {:?} {:?} {:?}", t.kind(), t.text_range(), t.text()).unwrap();
            }
        }
        out.push_str("}\n");
    }
    let parsed = if sdbl { crate::parse_sdbl(input) } else { crate::parse(input) };
    let mut out = String::new();
    node(&mut out, parsed.syntax_node());
    out.push_str(&format!("ERRORS {:?}\n", parsed.errors()));
    out
}

fn comparison_cases() -> Vec<(String, String, bool)> {
    let mut cases = Vec::new();
    for w in WITNESSES {
        cases.push((
            format!("witness/{:?}", w.kind),
            w.input.to_owned(),
            w.channel == Channel::Sdbl,
        ));
    }
    for word in all_words() {
        let witness = witness_of(word.kind);
        for spelling in [word.ru, word.en] {
            for variant in case_variants(spelling) {
                let input: String = lexer::tokenize(witness.input)
                    .iter()
                    .map(|t| if t.kind == word.kind { variant.as_str() } else { &t.text })
                    .collect();
                cases.push((format!("word/{}/{variant}", word.section), input, false));
            }
        }
    }
    for (ru, en) in PREPROCESSOR_SYMBOLS {
        for spelling in [ru, en] {
            for variant in case_variants(spelling) {
                cases.push((
                    format!("symbol/{variant}"),
                    format!("#Если {variant} Тогда\nПерем А;\n#КонецЕсли"),
                    false,
                ));
            }
        }
    }
    for f in FORMS {
        let input = match f.kinds[0] {
            TokenKind::String
            | TokenKind::StringStart
            | TokenKind::Date
            | TokenKind::Decimal
            | TokenKind::Float
            | TokenKind::Ident
            | TokenKind::Question => {
                format!("Процедура П() А = {}; КонецПроцедуры", f.text)
            }
            TokenKind::Minus if f.text == "-1" => "Процедура П() А = -1; КонецПроцедуры".to_owned(),
            TokenKind::Tilde if f.text == "~М:" => {
                "Процедура П() ~М: Возврат; КонецПроцедуры".to_owned()
            }
            TokenKind::Comment => format!("{}\nПроцедура П() КонецПроцедуры", f.text),
            _ => witness_of(f.kinds[0]).input.to_owned(),
        };
        assert!(input.contains(f.text), "форма {:?} отсутствует в конструкции", f.text);
        cases.push((format!("form/{}/{:?}", f.section, f.text), input, false));
    }
    for (spelling, kind, _) in ALLOWANCES {
        let witness = witness_of(*kind);
        for variant in case_variants(spelling) {
            let input: String = lexer::tokenize(witness.input)
                .iter()
                .map(|t| if t.kind == *kind { variant.as_str() } else { &t.text })
                .collect();
            cases.push((format!("allowance/{variant}"), input, false));
        }
    }
    for text in [
        "Тогда",
        "Процедура П() А; КонецПроцедуры",
        "#Неизвестная",
        "#Use М",
        "#Использовать М",
        "&1",
        "#",
        "&",
        "!",
        "{",
        "}",
        "|",
        "Процедура П() А = 'незакрыто; КонецПроцедуры",
        "Procedure P(",
    ] {
        cases.push((format!("invalid/{text:?}"), text.to_owned(), false));
    }
    cases
}

#[test]
fn structural_comparison_detects_a_changed_operator() {
    let plus = "Процедура П() А = Б + В; КонецПроцедуры";
    let minus = plus.replace('+', "-");
    let before = structural_dump(plus, false);
    let after = structural_dump(&minus, false);
    assert!(before.contains("T PLUS "));
    assert!(after.contains("T MINUS "));
    assert_ne!(before, after, "сравнение должно различать вид токена, а не только размер дерева");
    assert_eq!(before, structural_dump(plus, false));
}

/// Внешний зонд собирается с другой версией parser и принимает BSL/SDBL
/// через stdin. Дамп содержит полные тексты токенов без усечения, диапазоны,
/// границы вложенных узлов и ошибки; совпадения восстановленного текста мало.
#[test]
#[ignore = "нужна отдельная сборка: BSL_TOKEN_BASELINE_DUMP"]
fn parse_trees_match_another_build() {
    use std::io::Write;
    let executable =
        std::env::var_os("BSL_TOKEN_BASELINE_DUMP").expect("BSL_TOKEN_BASELINE_DUMP не задан");
    let baseline = |input: &str, sdbl: bool| {
        let mut child = std::process::Command::new(&executable)
            .arg(if sdbl { "sdbl" } else { "bsl" })
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("зонд запускается");
        child.stdin.take().unwrap().write_all(input.as_bytes()).expect("вход записан");
        let output = child.wait_with_output().expect("зонд завершён");
        assert!(
            output.status.success(),
            "зонд: {:?}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).expect("дамп в UTF-8")
    };
    let plus = "Процедура П() А = Б + В; КонецПроцедуры";
    assert_ne!(baseline(plus, false), baseline(&plus.replace('+', "-"), false));

    let mut cases = comparison_cases();
    let constructed = cases.len();
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let listed = std::process::Command::new("git")
        .args(["ls-files", "-z", "*.bsl"])
        .current_dir(&workspace)
        .output()
        .expect("git ls-files запускается");
    assert!(listed.status.success(), "git ls-files: {:?}", listed.status);
    let mut fixtures = 0;
    for path in String::from_utf8(listed.stdout).expect("пути в UTF-8").split_terminator('\0')
    {
        let input =
            std::fs::read_to_string(workspace.join(path)).expect("фикстура читается в UTF-8");
        cases.push((format!("fixture/{path}"), input, false));
        fixtures += 1;
    }
    assert!(fixtures > 0, "репозиторные фикстуры не найдены");
    for (name, input, sdbl) in cases {
        assert_eq!(structural_dump(&input, sdbl), baseline(&input, sdbl), "{name}");
    }
    println!("два разбора: {constructed} конструкций/неверных входов, {fixtures} репозиторных .bsl; различий 0; контроль +/- различается");
}
