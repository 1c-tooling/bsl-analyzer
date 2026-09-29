//! Markdown projection of method documentation, including every documented field and type.

use hir::{MethodDocs, ParameterDoc, TypeDoc};

/// Blank lines separate sections in Markdown; plain newlines would collapse into one paragraph.
pub(super) fn append_method_docs(markup: &mut String, docs: &MethodDocs) {
    append_section(markup, "Устарела", docs.deprecation.as_deref());
    append_section(markup, "Назначение", docs.purpose.as_deref());
    append_section(markup, "См. также", docs.link.as_deref());

    if !docs.parameters.is_empty() {
        markup.push_str("**Параметры:**\n\n");
        append_parameters(markup, &docs.parameters, 0);
        markup.push('\n');
    }
    if !docs.returned_value.is_empty() {
        markup.push_str("**Возвращаемое значение:**");
        if docs.returned_value.len() == 1 {
            markup.push(' ');
            append_type(markup, &docs.returned_value[0], 0, false);
        } else {
            markup.push_str("\n\n");
            append_types(markup, &docs.returned_value, 0);
        }
        markup.push('\n');
    }
    append_examples(markup, "Примеры", &docs.examples);
    append_examples(markup, "Варианты вызова", &docs.call_options);
}

/// Empty sections add no vertical space to short method tooltips.
fn append_section(markup: &mut String, title: &str, text: Option<&str>) {
    if let Some(text) = text.filter(|text| !text.is_empty()) {
        markup.push_str(&format!("**{title}:** {}\n\n", compact_prose(text)));
    }
}

/// Single-type parameters fit on one line; unions retain their visible ownership hierarchy.
fn append_parameters(markup: &mut String, parameters: &[ParameterDoc], depth: usize) {
    let indent = "  ".repeat(depth);
    for parameter in parameters {
        markup.push_str(&format!("{indent}- **{}**", inline_code(&parameter.name)));
        if parameter.types.len() == 1 {
            markup.push_str(": ");
            append_type(markup, &parameter.types[0], depth + 1, false);
        } else if parameter.types.is_empty() {
            markup.push('\n');
        } else {
            markup.push_str(":\n");
            append_types(markup, &parameter.types, depth + 1);
        }
    }
}

/// Descriptions stay with their type alternative, and fields stay under their container.
fn append_types(markup: &mut String, types: &[TypeDoc], depth: usize) {
    for ty in types.iter().filter(|ty| !ty.name.trim().is_empty()) {
        append_type(markup, ty, depth, true);
    }
}

/// A type and its prose form one readable line and let the editor wrap it naturally.
fn append_type(markup: &mut String, ty: &TypeDoc, depth: usize, list_item: bool) {
    if list_item {
        markup.push_str(&format!("{}- ", "  ".repeat(depth)));
    }

    let (name, description) = display_type(ty);
    markup.push_str(&inline_code(&name));
    if let Some(description) = description {
        markup.push_str(" — ");
        markup.push_str(&description);
    }
    markup.push('\n');

    if !ty.parameters.is_empty() {
        append_parameters(markup, &ty.parameters, depth + usize::from(list_item));
    }
}

/// Collection element declarations are part of the displayed type, even though the semantic
/// model retains them in the description for type inference.
fn display_type(ty: &TypeDoc) -> (String, Option<String>) {
    let Some(description) = ty.description.as_deref().filter(|text| !text.trim().is_empty()) else {
        return (ty.name.clone(), None);
    };
    let description = compact_prose(description);
    let lower_name = ty.name.to_lowercase();
    let collection_prefix = match lower_name.as_str() {
        "массив" | "фиксированныймассив" | "соответствие" | "структура" => {
            "из "
        }
        "array" | "fixedarray" | "map" | "structure" => "of ",
        _ => return (ty.name.clone(), Some(description)),
    };
    let lower_description = description.to_lowercase();
    if !lower_description.starts_with(collection_prefix) {
        return (ty.name.clone(), Some(description));
    }

    match description.split_once(" - ") {
        Some((element, prose)) => (format!("{} {element}", ty.name), Some(prose.trim().to_owned())),
        None => (format!("{} {description}", ty.name), None),
    }
}

/// Source wrapping is layout, not paragraph structure inside a type description.
fn compact_prose(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// One code block preserves the order of example lines without turning each into a list item.
fn append_examples(markup: &mut String, title: &str, lines: &[String]) {
    if lines.is_empty() {
        return;
    }
    let example = lines.join("\n");
    let fence = "`".repeat(longest_backtick_run(&example).max(2) + 1);
    markup.push_str(&format!("**{title}:**\n\n{fence}bsl\n{example}\n{fence}\n\n"));
}

/// A delimiter longer than the contents keeps literal names from changing Markdown structure.
fn inline_code(text: &str) -> String {
    let delimiter = "`".repeat(longest_backtick_run(text) + 1);
    if text.starts_with('`') || text.ends_with('`') {
        format!("{delimiter} {text} {delimiter}")
    } else {
        format!("{delimiter}{text}{delimiter}")
    }
}

/// Both inline code and fenced examples must tolerate backticks in documentation.
fn longest_backtick_run(text: &str) -> usize {
    text.split(|character| character != '`').map(str::len).max().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use expect_test::expect;

    /// Nested data and Markdown punctuation must not flatten the rendered field hierarchy.
    #[test]
    fn nested_fields_and_literal_type_names() {
        let mut docs = MethodDocs::empty();
        docs.returned_value = vec![TypeDoc::structured(
            "Structure".into(),
            None,
            vec![ParameterDoc::new(
                "Nested_Field".into(),
                vec![TypeDoc::structured(
                    "Structure".into(),
                    None,
                    vec![ParameterDoc::new(
                        "Name".into(),
                        vec![TypeDoc::simple("String".into(), Some("Name of the item".into()))],
                    )],
                )],
            )],
        )];
        let mut markup = String::new();
        append_method_docs(&mut markup, &docs);
        expect![[r#"
            **Возвращаемое значение:** `Structure`
            - **`Nested_Field`**: `Structure`
              - **`Name`**: `String` — Name of the item

        "#]]
        .assert_eq(&markup);
        assert_eq!(inline_code("`literal`"), "`` `literal` ``");
    }

    /// Code examples and call options stay separate from prose and cannot close their own fence.
    #[test]
    fn documentation_sections_and_examples() {
        let mut docs = MethodDocs::empty();
        docs.purpose = Some("Описание метода.".into());
        docs.deprecation = Some("Используйте НовыйМетод.".into());
        docs.link = Some("См. ОбщийМодуль.НовыйМетод".into());
        docs.examples = vec!["Сообщить(\"```\");".into(), "Сообщить(\"Готово\");".into()];
        docs.call_options = vec!["НовыйМетод();".into()];
        let mut markup = String::new();
        append_method_docs(&mut markup, &docs);
        expect![[r#"
            **Устарела:** Используйте НовыйМетод.

            **Назначение:** Описание метода.

            **См. также:** См. ОбщийМодуль.НовыйМетод

            **Примеры:**

            ````bsl
            Сообщить("```");
            Сообщить("Готово");
            ````

            **Варианты вызова:**

            ```bsl
            НовыйМетод();
            ```

        "#]]
        .assert_eq(&markup);
    }

    /// Collection element syntax belongs to the type label while prose remains its description.
    #[test]
    fn collection_element_is_displayed_as_part_of_type() {
        let ty = TypeDoc::simple(
            "ФиксированныйМассив".into(),
            Some("из Строка - имена реквизитов".into()),
        );
        assert_eq!(
            display_type(&ty),
            ("ФиксированныйМассив из Строка".into(), Some("имена реквизитов".into()))
        );

        let correspondence = TypeDoc::simple(
            "Соответствие".into(),
            Some("из КлючИЗначение - список объектов".into()),
        );
        assert_eq!(
            display_type(&correspondence),
            ("Соответствие из КлючИЗначение".into(), Some("список объектов".into()))
        );
    }
}
