//! Source ranges for structured documentation, independent of editor colors.

use stdx::case::find_ignore_case;
use syntax::{TextRange, TextSize};

use super::{
    is_dotted_type_reference, is_likely_parameter_doc_name, is_likely_type_name,
    parse_collection_type, section_header, split_type_description, Section,
};

/// A structural role inside a method's documentation comment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocCommentTokenKind {
    Keyword,
    Parameter,
    Property,
    Type,
    Reference,
}

/// A byte range relative to one input line, excluding surrounding comment text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocCommentToken {
    pub line: usize,
    pub range: TextRange,
    pub kind: DocCommentTokenKind,
}

/// Classifies documentation lines without `//`, preserving their original byte offsets.
///
/// These are syntactic roles: a colored type or reference need not resolve to a symbol.
/// Prose and example code intentionally have no structural tokens.
pub fn doc_comment_tokens(lines: &[&str]) -> Vec<DocCommentToken> {
    let mut tokens = Vec::new();
    let mut section = None;
    let mut prev_blank = false;
    let mut has_entry = false;
    for (index, raw) in lines.iter().enumerate() {
        // Tabs and spaces have the same byte width; this matches the documentation
        // parser's separators without changing any source positions.
        let normalized = raw.replace('\t', " ");
        let line = normalized.trim();
        let mut output = LineTokens { line: index, source: &normalized, tokens: &mut tokens };
        if let Some((next, payload)) =
            section_header(line, section == Some(Section::Parameters), prev_blank)
        {
            section = Some(next);
            has_entry = false;
            if next == Section::Deprecated {
                for marker in ["устарела", "deprecated"] {
                    if let Some(range) = find_ignore_case(line, marker) {
                        output.push(&line[range], DocCommentTokenKind::Keyword);
                        break;
                    }
                }
            } else if let Some(payload) = payload {
                let start = line.len() - payload.len();
                output.push(line[..start].trim_end(), DocCommentTokenKind::Keyword);
                has_entry = output.types(&line[start..]);
            } else {
                let header = line.find(':').map_or(line, |end| &line[..=end]);
                output.push(header, DocCommentTokenKind::Keyword);
            }
        } else if matches!(section, Some(Section::Parameters | Section::Returns)) {
            if let Some(field) = field_payload(line).filter(|_| has_entry) {
                output.entry(field, DocCommentTokenKind::Property);
            } else if section == Some(Section::Parameters) {
                if has_entry
                    && (line.starts_with('-')
                        || is_dotted_type_reference(
                            split_type_description(line).map_or(line, |(ty, _)| ty),
                        ))
                {
                    output.types(line.trim_start_matches('-').trim_start());
                } else if output.entry(line, DocCommentTokenKind::Parameter) {
                    has_entry = true;
                }
            } else if !line.starts_with('*')
                && output.types(line.trim_start_matches('-').trim_start())
            {
                has_entry = true;
            }
        }
        if !matches!(section, Some(Section::Examples | Section::CallOptions)) {
            output.references(line);
        }
        prev_blank = line.is_empty();
    }
    tokens.sort_by_key(|token| (token.line, token.range.start()));
    let mut previous = None;
    tokens.retain(|token| {
        if previous.is_some_and(|(line, end)| line == token.line && end > token.range.start()) {
            return false;
        }
        previous = Some((token.line, token.range.end()));
        true
    });
    tokens
}

/// Every leading star is structural; the remaining payload has ordinary field syntax.
fn field_payload(line: &str) -> Option<&str> {
    let payload = line.trim_start_matches('*');
    (payload.len() < line.len()).then(|| payload.trim_start())
}

struct LineTokens<'a> {
    line: usize,
    source: &'a str,
    tokens: &'a mut Vec<DocCommentToken>,
}

impl LineTokens<'_> {
    /// All parts are slices of `source`, so repeated names keep distinct positions.
    fn push(&mut self, part: &str, kind: DocCommentTokenKind) {
        if part.is_empty() {
            return;
        }
        let start = part.as_ptr() as usize - self.source.as_ptr() as usize;
        let range = TextRange::at(TextSize::from(start as u32), TextSize::of(part));
        self.tokens.push(DocCommentToken { line: self.line, range, kind });
    }

    /// A named entry requires the same space-flanked separator as method docs.
    fn entry(&mut self, line: &str, kind: DocCommentTokenKind) -> bool {
        let Some((name, rest)) = line.split_once(" - ") else { return false };
        let name = name.trim();
        if !is_likely_parameter_doc_name(name) {
            return false;
        }
        self.push(name, kind);
        self.types(rest.trim());
        true
    }

    /// Only the type slot is classified; descriptions after the dash remain prose.
    fn types(&mut self, line: &str) -> bool {
        let slot = split_type_description(line)
            .map_or(line, |(ty, _)| ty)
            .trim()
            .trim_end_matches(':')
            .trim();
        self.references(slot);
        let mut found = false;
        for member in slot.split(',').map(str::trim).filter(|part| !part.is_empty()) {
            if is_likely_type_name(member) {
                self.push(member, DocCommentTokenKind::Type);
                found = true;
            } else if parse_collection_type(member).is_some() {
                for marker in [" из ", " of "] {
                    if let Some(range) = find_ignore_case(member, marker) {
                        self.push(member[..range.start].trim(), DocCommentTokenKind::Type);
                        self.push(member[range.clone()].trim(), DocCommentTokenKind::Keyword);
                        self.types(member[range.end..].trim());
                        found = true;
                        break;
                    }
                }
            }
        }
        found
    }

    /// Uses the documentation parser's reference boundaries, including unit guards.
    fn references(&mut self, line: &str) {
        for (marker, target) in super::see_reference_ranges(line) {
            self.push(&line[marker], DocCommentTokenKind::Keyword);
            self.push(&line[target], DocCommentTokenKind::Reference);
        }
    }
}
