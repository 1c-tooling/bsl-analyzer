/// A numbered parameter group stored as one entry in the platform reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParameterSeries<'a> {
    pub prefix: &'a str,
    pub first: u32,
    pub last: Option<u32>,
}

impl<'a> ParameterSeries<'a> {
    /// Reads the reference's `Value1-Value10` and `Value1,...,ValueN` forms.
    pub fn parse(name: &'a str) -> Option<Self> {
        let (head, tail) = name.split_once(",...,").or_else(|| name.split_once('-'))?;
        let (prefix, first) = numbered_name(head)?;
        let suffix = tail.trim_start().strip_prefix(prefix)?;
        let last = if !suffix.is_empty() && suffix.chars().all(|c| c.is_alphabetic()) {
            None
        } else if !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit()) {
            Some(suffix.parse::<u32>().ok()?)
        } else {
            return None;
        };
        if last.is_some_and(|last| last < first) {
            return None;
        }
        Some(Self { prefix, first, last })
    }

    /// Reads the first name of an explicitly variadic numbered parameter.
    pub fn from_numbered_name(name: &'a str) -> Option<Self> {
        let (prefix, first) = numbered_name(name)?;
        Some(Self { prefix, first, last: None })
    }

    /// Names one argument relative to the start of this group, within its bound.
    pub fn name_at(self, offset: usize) -> Option<String> {
        let number = self.first.checked_add(u32::try_from(offset).ok()?)?;
        if self.last.is_some_and(|last| number > last) {
            return None;
        }
        Some(format!("{}{number}", self.prefix))
    }
}

/// Splits a numbered name without treating digits inside its prefix as an index.
fn numbered_name(name: &str) -> Option<(&str, u32)> {
    let prefix = name.trim_end_matches(|c: char| c.is_ascii_digit());
    if prefix.is_empty() || prefix.len() == name.len() {
        return None;
    }
    Some((prefix, name[prefix.len()..].parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Both documented separators describe the same bounded sequence.
    #[test]
    fn bounded_series_names_each_argument() {
        for name in ["Значение1-Значение10", "Значение1,...,Значение10"]
        {
            let series = ParameterSeries::parse(name).unwrap();
            assert_eq!(series.name_at(0).as_deref(), Some("Значение1"));
            assert_eq!(series.name_at(9).as_deref(), Some("Значение10"));
            assert_eq!(series.name_at(10), None);
        }
        let series = ParameterSeries::parse("Value3-Value5").unwrap();
        assert_eq!(series.name_at(0).as_deref(), Some("Value3"));
        assert_eq!(series.name_at(2).as_deref(), Some("Value5"));
        assert_eq!(series.name_at(3), None);
    }

    /// An alphabetic endpoint denotes an unbounded group in platform data.
    #[test]
    fn unbounded_series_and_explicit_numbered_names() {
        for name in ["Содержимое1,...,СодержимоеN", "Содержимое1-СодержимоеК"]
        {
            let series = ParameterSeries::parse(name).unwrap();
            assert_eq!(series.last, None);
            assert_eq!(series.name_at(11).as_deref(), Some("Содержимое12"));
        }
        let series = ParameterSeries::from_numbered_name("КоличествоЭлементов1").unwrap();
        assert_eq!(series.name_at(2).as_deref(), Some("КоличествоЭлементов3"));
    }

    /// A hyphen in an ordinary parameter name must not create a variadic group.
    #[test]
    fn rejects_unrelated_names_and_invalid_bounds() {
        for name in [
            "Имя",
            "Имя-Фамилия",
            "Значение-Значение10",
            "X1,...,Y2",
            "X1,...,X-",
            "X1,...,X-end-",
            "Value3-Value1",
            "Value1-Value+3",
            "Value1-Value4294967296",
        ] {
            assert_eq!(ParameterSeries::parse(name), None, "{name}");
        }
        assert_eq!(ParameterSeries::from_numbered_name("Значения"), None);
        assert_eq!(ParameterSeries::from_numbered_name("1"), None);
        assert_eq!(ParameterSeries::from_numbered_name("Value4294967296"), None);
        let series = ParameterSeries::from_numbered_name("Value4294967295").unwrap();
        assert_eq!(series.name_at(1), None);
    }
}
