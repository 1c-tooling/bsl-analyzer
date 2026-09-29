use std::borrow::Cow;

use crate::domain::SignatureParam;

/// Names an actual argument without expanding the compact platform signature.
///
/// The final slot may represent `Значение1-Значение10`, `Значение1,...,ЗначениеN`,
/// or an explicitly variadic parameter. Fixed slots retain their source names.
pub fn parameter_name_for_argument(
    params: &[SignatureParam],
    index: usize,
) -> Option<Cow<'_, str>> {
    let (last, fixed) = params.split_last()?;
    if index < fixed.len() {
        return Some(Cow::Borrowed(fixed[index].name.as_str()));
    }

    let repetition = index - fixed.len();
    if let Some(series) = NumberedParameter::parse(&last.name, last.is_variadic) {
        return series.name(repetition).map(Cow::Owned);
    }

    (repetition == 0 || last.is_variadic).then_some(Cow::Borrowed(last.name.as_str()))
}

struct NumberedParameter<'a> {
    prefix: &'a str,
    first: usize,
    last: Option<usize>,
}

impl<'a> NumberedParameter<'a> {
    /// A numbered name repeats only when a range or platform metadata declares it.
    fn parse(name: &'a str, is_variadic: bool) -> Option<Self> {
        let range = name.split_once(",...,").or_else(|| name.split_once('-'));
        let (head, tail) = match range {
            Some((head, tail)) => (head.trim(), Some(tail.trim())),
            None if is_variadic => (name, None),
            None => return None,
        };
        let prefix = head.trim_end_matches(|c: char| c.is_ascii_digit());
        if prefix.is_empty() {
            return None;
        }
        let first = head[prefix.len()..].parse::<usize>().ok()?;
        let last = match tail {
            Some(tail) => {
                let suffix = tail.strip_prefix(prefix)?;
                if !suffix.is_empty() && suffix.chars().all(|c| c.is_alphabetic()) {
                    None
                } else {
                    let last = suffix.parse::<usize>().ok()?;
                    if last < first {
                        return None;
                    }
                    Some(last)
                }
            }
            None => None,
        };
        Some(Self { prefix, first, last })
    }

    /// A bounded range stops at its declared endpoint instead of inventing extra parameters.
    fn name(&self, repetition: usize) -> Option<String> {
        let number = self.first.checked_add(repetition)?;
        if self.last.is_some_and(|last| number > last) {
            return None;
        }
        Some(format!("{}{number}", self.prefix))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Presentation tests use source metadata without needing a workspace or type inference.
    fn param(name: &str, is_variadic: bool) -> SignatureParam {
        SignatureParam {
            name: name.into(),
            types: Vec::new(),
            is_optional: false,
            is_variadic,
            default_value: None,
            description: None,
            is_val: false,
        }
    }

    /// Numbering is relative to the repeated slot, including a range that starts above one.
    #[test]
    fn numbered_ranges_preserve_prefix_start_and_bound() {
        for name in ["Значение3-Значение5", "Value3,...,Value5"] {
            let params = [param("Шаблон", false), param(name, false)];
            let prefix = if name.starts_with("Value") { "Value" } else { "Значение" };
            assert_eq!(parameter_name_for_argument(&params, 0).as_deref(), Some("Шаблон"));
            for (index, number) in [(1, 3), (2, 4), (3, 5)] {
                assert_eq!(
                    parameter_name_for_argument(&params, index).as_deref(),
                    Some(format!("{prefix}{number}").as_str()),
                );
            }
            assert_eq!(parameter_name_for_argument(&params, 4), None);
        }
    }

    /// Both source ranges and explicitly variadic numbered slots support an open-ended tail.
    #[test]
    fn unbounded_series_label_each_argument() {
        for (name, variadic) in [
            ("Значение1,...,ЗначениеN", false),
            ("Value1-ValueN", false),
            ("КоличествоЭлементов1", true),
        ] {
            let params = [param(name, variadic)];
            let prefix = name.split_once('1').unwrap().0;
            assert_eq!(
                parameter_name_for_argument(&params, 11).as_deref(),
                Some(format!("{prefix}12").as_str()),
            );
        }
    }

    /// A normal numbered parameter does not imply that later undeclared arguments exist.
    #[test]
    fn fixed_and_unnumbered_variadic_parameters_keep_their_names() {
        let fixed = [param("Значение1", false)];
        assert_eq!(parameter_name_for_argument(&fixed, 0).as_deref(), Some("Значение1"));
        assert_eq!(parameter_name_for_argument(&fixed, 1), None);
        let variadic = [param("Значения", true)];
        assert_eq!(parameter_name_for_argument(&variadic, 3).as_deref(), Some("Значения"));
        assert_eq!(parameter_name_for_argument(&[], 0), None);
    }

    /// Malformed or mismatched ranges cannot manufacture labels for undeclared arguments.
    #[test]
    fn malformed_ranges_and_overflow_stay_unexpanded() {
        for name in ["Имя-Фамилия", "Value1-Other10", "Value5-Value3", "1-10", "Value-ValueN"]
        {
            let params = [param(name, false)];
            assert_eq!(parameter_name_for_argument(&params, 0).as_deref(), Some(name));
            assert_eq!(parameter_name_for_argument(&params, 1), None);
        }
        let params = [param("Value1-ValueN", false)];
        assert_eq!(parameter_name_for_argument(&params, usize::MAX), None);
    }
}
