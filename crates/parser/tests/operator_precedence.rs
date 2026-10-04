//! Порядок вычисления выражений совпадает с таблицей 4.5.4.
//!
//! Таблица перечисляет операции **в порядке увеличения приоритета** и говорит,
//! что операции с одинаковым приоритетом вычисляются **слева направо**.
//! Направление здесь закрепляется утверждениями, а не формой цепочки вызовов:
//! раздел 4.5.3 даёт свою таблицу уровней логических операций в обратную
//! сторону («Уровень 1 — скобки … Уровень 4 — Или»), а `v8std` перевёрнут
//! относительно 4.5.4 третьим способом. Гейт, не называющий направление, зелен
//! при перевёрнутой цепочке.
//!
//! Утверждение выбрано так, чтобы оно не проходило вхолостую: проверяется, КАКАЯ
//! операция стоит снаружи, а не то, что дерево вообще ветвится. Снаружи стоит
//! самая слабая — в этом и состоит приоритет.
//!
//! Свидетели соседних уровней дополняются проверками равного приоритета.
//! Граница «унарные `+`/`-` против `.` и `()`» НЕ наблюдаема как связывание:
//! унарный знак узла не строит, поэтому оба порядка дают
//! буквально одно дерево `EXPR { MINUS, FIELD_EXPR { … } }`, и различать нечего.
//! Мутант, меняющий эти два уровня местами, оставляет проверку зелёной целиком.
//! Оговорка снимается вместе с задачей о собственном узле унарного знака:
//! `https://github.com/itrous/bsl-analyzer/issues/51`.
//!
//! Provenance: `docs/legal/bsl-clean-room-slice-b3.md`.

use syntax::{SyntaxKind, SyntaxNode};

/// Знаки операций выражения. Знак присваивания сюда не входит: поиск начинается
/// с правой части, где его уже нет.
const OPERATORS: &[SyntaxKind] = &[
    SyntaxKind::KW_OR,
    SyntaxKind::KW_AND,
    SyntaxKind::KW_NOT,
    SyntaxKind::EQ,
    SyntaxKind::NEQ,
    SyntaxKind::LT,
    SyntaxKind::LE,
    SyntaxKind::GT,
    SyntaxKind::GE,
    SyntaxKind::PLUS,
    SyntaxKind::MINUS,
    SyntaxKind::STAR,
    SyntaxKind::SLASH,
    SyntaxKind::PERCENT,
    SyntaxKind::DOT,
    SyntaxKind::L_BRACKET,
];

/// Правая часть присваивания — то самое выражение, о котором идёт речь.
fn right_hand_side(input: &str) -> SyntaxNode {
    let parsed = parser::parse(input);
    assert!(!parsed.has_errors(), "{input:?}: {:?}", parsed.errors());
    let root = parsed.syntax_node();
    let assign = root
        .descendants()
        .find(|node| node.kind() == SyntaxKind::ASSIGN_STMT)
        .unwrap_or_else(|| panic!("во входе {input:?} нет присваивания"));

    assign
        .children()
        .filter(|child| child.kind() == SyntaxKind::EXPR)
        .last()
        .unwrap_or_else(|| panic!("у присваивания во входе {input:?} нет правой части"))
}

/// Знак самой внешней операции выражения.
///
/// Обход в ширину: самый мелкий узел, у которого есть свой знак, и есть самая
/// слабо связывающая операция. Унарные плюс и минус узла не строят и держат
/// знак прямо в `EXPR`, поэтому ищется знак среди СОБСТВЕННЫХ токенов узла, а
/// не вид узла.
fn outermost_operator(input: &str) -> SyntaxKind {
    let mut level = vec![right_hand_side(input)];

    while !level.is_empty() {
        let mut next = Vec::new();

        for node in &level {
            if node.kind() == SyntaxKind::CALL_EXPR {
                return SyntaxKind::L_PAREN;
            }
            for element in node.children_with_tokens() {
                if let Some(token) = element.as_token() {
                    if OPERATORS.contains(&token.kind()) {
                        return token.kind();
                    }
                }
            }
        }

        for node in &level {
            next.extend(node.children());
        }

        level = next;
    }

    panic!("во входе {input:?} не нашлось ни одного знака операции")
}

/// Наблюдаемые пары уровней 4.5.4 согласны с таблицей.
///
/// Слева в паре — операция более слабая по таблице; она и обязана оказаться
/// снаружи. Каждый вход содержит обе операции пары, иначе утверждение было бы
/// зелено при любой реализации.
#[test]
fn the_precedence_ladder_matches_section_4_5_4() {
    let cases: &[(&str, SyntaxKind, &str)] = &[
        ("Х = а Или б И в;", SyntaxKind::KW_OR, "Или слабее И"),
        ("Х = Не а И б;", SyntaxKind::KW_AND, "И слабее Не"),
        ("Х = Не а = б;", SyntaxKind::KW_NOT, "Не слабее сравнения"),
        ("Х = а = б + в;", SyntaxKind::EQ, "сравнение слабее сложения"),
        ("Х = а + б * в;", SyntaxKind::PLUS, "сложение слабее умножения"),
        ("Х = -а * б;", SyntaxKind::STAR, "умножение слабее унарного минуса"),
        ("Х = а <> б - в;", SyntaxKind::NEQ, "сравнение слабее вычитания"),
        ("Х = а < б / в;", SyntaxKind::LT, "сравнение слабее деления"),
        ("Х = а <= б % в;", SyntaxKind::LE, "сравнение слабее остатка"),
        ("Х = а > б + в;", SyntaxKind::GT, "сравнение слабее сложения"),
        ("Х = а >= б * в;", SyntaxKind::GE, "сравнение слабее умножения"),
        ("Х = +а / б;", SyntaxKind::SLASH, "деление слабее унарного плюса"),
        ("Х = а + Ф();", SyntaxKind::PLUS, "сложение слабее вызова"),
        ("Х = (а Или б) И в;", SyntaxKind::KW_AND, "скобки меняют порядок"),
    ];

    let mut breaches = Vec::new();

    for (input, expected, note) in cases {
        let actual = outermost_operator(input);
        if actual != *expected {
            breaches.push(format!(
                "{note}: во входе {input:?} снаружи ожидался {expected:?}, а стоит {actual:?}"
            ));
        }
    }

    assert!(
        breaches.is_empty(),
        "порядок разошёлся с таблицей 4.5.4:\n  {}",
        breaches.join("\n  ")
    );
}

/// Операции одного приоритета вычисляются слева направо.
///
/// Вычитание различает две скобковки численным результатом; сравнение имеет
/// отдельный свидетель, потому что его цепочку строит другой цикл.
#[test]
fn operations_of_equal_precedence_associate_to_the_left() {
    assert_left_nested("Х = а - б - в;", SyntaxKind::MINUS, "а - б", "в");
}

#[test]
fn comparisons_of_equal_precedence_associate_to_the_left() {
    assert_left_nested("Х = а = б = в;", SyntaxKind::EQ, "а = б", "в");
}

fn assert_left_nested(input: &str, operator: SyntaxKind, left: &str, right: &str) {
    let rhs = right_hand_side(input);

    let outer = rhs
        .descendants()
        .find(|node| node.kind() == SyntaxKind::BINARY_EXPR)
        .expect("во входе нет двоичного выражения");

    assert_eq!(direct_operator(&outer), Some(operator));
    let left_operand = outer.children().next().expect("у двоичного выражения нет левой части");
    assert_eq!(left_operand.text().to_string().trim(), left, "{input}");
    let nested = left_operand
        .descendants()
        .find(|node| node.kind() == SyntaxKind::BINARY_EXPR)
        .expect("слева должно быть вложенное выражение");
    assert_eq!(direct_operator(&nested), Some(operator));

    assert!(
        left_operand.descendants().any(|node| node.kind() == SyntaxKind::BINARY_EXPR),
        "левая ассоциативность нарушена: вложенная операция оказалась не слева"
    );

    let right_operand = outer.children().last().expect("у двоичного выражения нет правой части");
    assert_eq!(right_operand.text().to_string().trim(), right, "{input}");

    assert!(
        !right_operand.descendants().any(|node| node.kind() == SyntaxKind::BINARY_EXPR),
        "левая ассоциативность нарушена: вложенная операция оказалась справа"
    );
}

fn direct_operator(node: &SyntaxNode) -> Option<SyntaxKind> {
    node.children_with_tokens()
        .filter_map(|element| element.into_token())
        .map(|token| token.kind())
        .find(|kind| OPERATORS.contains(kind))
}

#[test]
fn unary_signs_remain_siblings_of_field_and_call_without_a_unary_node() {
    for (input, sign, postfix) in [
        ("Х = -а.б;", SyntaxKind::MINUS, SyntaxKind::FIELD_EXPR),
        ("Х = +а();", SyntaxKind::PLUS, SyntaxKind::CALL_EXPR),
    ] {
        let rhs = right_hand_side(input);
        assert!(!rhs.descendants().any(|node| node.kind() == SyntaxKind::UNARY_EXPR));
        let holder = rhs
            .descendants()
            .find(|node| direct_operator(node) == Some(sign))
            .expect("знак должен остаться собственным токеном EXPR");
        assert_eq!(holder.kind(), SyntaxKind::EXPR);
        let operand = holder
            .children()
            .find(|node| node.kind() == postfix)
            .expect("постфиксный операнд должен быть соседом знака, не его владельцем");
        assert_eq!(operand.children().next().unwrap().kind(), SyntaxKind::IDENT);
        assert_eq!(operand.children().next().unwrap().text().to_string(), "а");
        assert!(
            observe_precedence(&rhs, false).mixed.iter().all(|row| row.iter().all(|n| *n == 0)),
            "эта граница не должна засчитываться как доказанная"
        );
    }
    let not = right_hand_side("Х = Не а.б;");
    assert!(not.descendants().any(|node| node.kind() == SyntaxKind::UNARY_EXPR));
}

fn operation_level(node: &SyntaxNode) -> Option<usize> {
    match node.kind() {
        // Индекс разделяет postfix-уровень по выбранному A, не по таблице 4.5.4.
        SyntaxKind::FIELD_EXPR | SyntaxKind::CALL_EXPR | SyntaxKind::INDEX_EXPR => Some(8),
        SyntaxKind::UNARY_EXPR if direct_operator(node) == Some(SyntaxKind::KW_NOT) => Some(3),
        SyntaxKind::EXPR
            if matches!(direct_operator(node), Some(SyntaxKind::PLUS | SyntaxKind::MINUS)) =>
        {
            Some(7)
        }
        SyntaxKind::BINARY_EXPR => match direct_operator(node)? {
            SyntaxKind::KW_OR => Some(1),
            SyntaxKind::KW_AND => Some(2),
            SyntaxKind::EQ
            | SyntaxKind::NEQ
            | SyntaxKind::LT
            | SyntaxKind::LE
            | SyntaxKind::GT
            | SyntaxKind::GE => Some(4),
            SyntaxKind::PLUS | SyntaxKind::MINUS => Some(5),
            SyntaxKind::STAR | SyntaxKind::SLASH | SyntaxKind::PERCENT => Some(6),
            _ => None,
        },
        _ => None,
    }
}

fn unwrapped_operation(mut node: SyntaxNode) -> Option<usize> {
    loop {
        if let Some(level) = operation_level(&node) {
            return Some(level);
        }
        if node.kind() != SyntaxKind::EXPR {
            return None;
        }
        let mut children = node.children();
        let child = children.next()?;
        if children.next().is_some() {
            return None;
        }
        node = child;
    }
}

#[derive(Default, Debug)]
struct Observation {
    mixed: [[usize; 8]; 8],
    breaches: usize,
    unary_postfix_excluded: usize,
}

fn observe_precedence(root: &SyntaxNode, reversed: bool) -> Observation {
    let mut result = Observation::default();
    for node in root.descendants() {
        let Some(parent) = operation_level(&node) else { continue };
        for (index, child) in node.children().enumerate() {
            // Аргументы вызова имеют свою группировку, не приоритет получателя.
            if parent == 8 && index != 0 {
                continue;
            }
            let Some(child) = unwrapped_operation(child) else { continue };
            if parent == child {
                continue;
            }
            if (parent, child) == (7, 8) {
                result.unary_postfix_excluded += 1;
                continue;
            }
            result.mixed[parent - 1][child - 1] += 1;
            if if reversed { parent < child } else { parent > child } {
                result.breaches += 1;
            }
        }
    }
    result
}

#[test]
fn the_corpus_observer_distinguishes_the_table_from_its_reverse_and_respects_groups() {
    let control = right_hand_side("Х = а Или б И в;");
    let normal = observe_precedence(&control, false);
    assert_eq!(normal.mixed[0][1], 1);
    assert_eq!(normal.breaches, 0);
    assert_eq!(observe_precedence(&control, true).breaches, 1);
    let grouped = right_hand_side("Х = (а Или б) И в;");
    let observed = observe_precedence(&grouped, false);
    assert_eq!(observed.breaches, 0);
    assert!(observed.mixed.iter().all(|row| row.iter().all(|n| *n == 0)));
    let call = right_hand_side("Х = Ф(а Или б И в);");
    assert_eq!(observe_precedence(&call, false).mixed[0][1], 1);
    for input in ["Х = а + Ф();", "Х = а + б[0];", "Х = а[0] + б;"] {
        let rhs = right_hand_side(input);
        assert_eq!(unwrapped_operation(rhs.clone()), Some(5), "{input}");
        let normal = observe_precedence(&rhs, false);
        assert_eq!(normal.mixed[4][7], 1, "{input}");
        assert_eq!(normal.breaches, 0, "{input}");
        assert_eq!(observe_precedence(&rhs, true).breaches, 1, "{input}");
    }
    assert_eq!(outermost_operator("Х = Ф();"), SyntaxKind::L_PAREN);
}

fn bsl_files_under(root: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(root).expect("корень корпуса читается") {
        let entry = entry.expect("запись каталога читается");
        let kind = entry.file_type().expect("вид записи читается");
        if kind.is_dir() {
            bsl_files_under(&entry.path(), files);
        } else if kind.is_file() && entry.path().extension().is_some_and(|ext| ext == "bsl") {
            files.push(entry.path());
        }
    }
}

#[test]
#[ignore = "частный корпус; корни задаются BSL_PRECEDENCE_CORPUS"]
fn corpus_precedence_report() {
    the_corpus_observer_distinguishes_the_table_from_its_reverse_and_respects_groups();
    println!("control=Или/И normal=0 reversed=1; unary/postfix excluded; groups respected");
    let mut groups = Vec::new();
    if let Some(roots) = std::env::var_os("BSL_PRECEDENCE_CORPUS") {
        for root in std::env::split_paths(&roots) {
            let mut files = Vec::new();
            bsl_files_under(&root, &mut files);
            groups.push((root.display().to_string(), files));
        }
    }
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let listed = std::process::Command::new("git")
        .args(["ls-files", "-z", "*.bsl"])
        .current_dir(&workspace)
        .output()
        .expect("git ls-files запускается");
    assert!(listed.status.success());
    let fixtures = String::from_utf8(listed.stdout)
        .expect("пути UTF-8")
        .split_terminator('\0')
        .map(|path| workspace.join(path))
        .collect();
    groups.push(("tracked fixtures".to_owned(), fixtures));
    let mut all_breaches = 0;
    let mut all_unreadable = 0;
    for (name, mut files) in groups {
        files.sort();
        files.dedup();
        let (mut read, mut unreadable, mut with_errors) = (0, 0, 0);
        let mut total = Observation::default();
        for file in files {
            let Ok(text) = std::fs::read_to_string(&file) else {
                unreadable += 1;
                continue;
            };
            read += 1;
            let parsed = parser::parse(&text);
            with_errors += usize::from(parsed.has_errors());
            let observed = observe_precedence(&parsed.syntax_node(), false);
            for i in 0..8 {
                for j in 0..8 {
                    total.mixed[i][j] += observed.mixed[i][j];
                }
            }
            total.breaches += observed.breaches;
            total.unary_postfix_excluded += observed.unary_postfix_excluded;
        }
        println!("{name}: read={read} unreadable={unreadable} with_errors={with_errors} {total:?}");
        all_breaches += total.breaches;
        all_unreadable += unreadable;
    }
    assert_eq!(all_unreadable, 0, "непрочитанные файлы корпуса");
    assert_eq!(all_breaches, 0, "наблюдаемые связывания расходятся с 4.5.4");
}
