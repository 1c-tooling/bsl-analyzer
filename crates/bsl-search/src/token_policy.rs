use crate::error::{EmbeddingFailure, EmbeddingFailureCode, SearchError};
use sha2::{Digest, Sha256};
use std::path::Path;
use tokenizers::Tokenizer;

pub const SEGMENTATION_VERSION: &str = "token-segments-v1";
pub(crate) const MAX_SOURCE_WINDOW_BYTES: usize = 32 * 1024;

/// A loaded tokenizer and its exact, immutable document token budget.
#[derive(Clone)]
pub struct TokenPolicy {
    tokenizer: Tokenizer,
    tokenizer_sha256: String,
    max_tokens: usize,
}

impl std::fmt::Debug for TokenPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenPolicy")
            .field("tokenizer_sha256", &self.tokenizer_sha256)
            .field("max_tokens", &self.max_tokens)
            .finish()
    }
}

impl TokenPolicy {
    pub fn load(
        path: &Path,
        expected_sha256: &str,
        max_tokens: usize,
    ) -> Result<Self, SearchError> {
        if max_tokens == 0 || expected_sha256.len() != 64 {
            return Err(invalid_config());
        }
        let bytes = std::fs::read(path).map_err(|_| invalid_config())?;
        let tokenizer_sha256 =
            Sha256::digest(&bytes).iter().map(|byte| format!("{byte:02x}")).collect::<String>();
        if !tokenizer_sha256.eq_ignore_ascii_case(expected_sha256) {
            return Err(invalid_config());
        }
        let mut tokenizer = Tokenizer::from_bytes(&bytes).map_err(|_| invalid_config())?;
        tokenizer.with_truncation(None).map_err(|_| invalid_config())?;
        tokenizer.with_padding(None);
        Ok(Self { tokenizer, tokenizer_sha256, max_tokens })
    }

    pub fn count(&self, input: &str) -> Result<usize, SearchError> {
        self.encode(input).map(|ids| ids.len())
    }

    pub fn encode(&self, input: &str) -> Result<Vec<u32>, SearchError> {
        self.tokenizer
            .encode(input, true)
            .map(|encoding| encoding.get_ids().to_vec())
            .map_err(|_| invalid_config())
    }

    pub fn check(&self, input: &str) -> Result<(), SearchError> {
        if self.count(input)? > self.max_tokens {
            Err(input_too_large())
        } else {
            Ok(())
        }
    }

    pub fn max_tokens(&self) -> usize {
        self.max_tokens
    }

    pub fn tokenizer_sha256(&self) -> &str {
        &self.tokenizer_sha256
    }

    pub fn segmentation_version(&self) -> &'static str {
        SEGMENTATION_VERSION
    }

    /// Split a source body using the caller's exact final-input and singleton-byte check.
    pub fn split_source(
        &self,
        source: &str,
        mut check: impl FnMut(u32, &str) -> Result<(), SearchError>,
    ) -> Result<Vec<std::ops::Range<usize>>, SearchError> {
        split_source(source, &mut check)
    }
}

fn split_source(
    source: &str,
    check: &mut impl FnMut(u32, &str) -> Result<(), SearchError>,
) -> Result<Vec<std::ops::Range<usize>>, SearchError> {
    if source.is_empty() {
        return Ok(Vec::new());
    }
    let mut ranges = Vec::new();
    let mut start = 0;
    while start < source.len() {
        let mut end = next_window_end(source, start);
        loop {
            let ordinal = ranges.len() as u32 + 1;
            match check(ordinal, &source[start..end]) {
                Ok(()) => {
                    ranges.push(start..end);
                    start = end;
                    break;
                }
                Err(error) if is_input_too_large(&error) => {
                    if end == next_char_end(source, start) {
                        return Err(error);
                    }
                    let midpoint = start + (end - start) / 2;
                    end = floor_char_boundary(source, midpoint);
                    if end <= start {
                        end = next_char_end(source, start);
                    }
                }
                Err(error) => return Err(error),
            }
        }
    }
    Ok(ranges)
}

fn next_window_end(source: &str, start: usize) -> usize {
    let limit = floor_char_boundary(source, (start + MAX_SOURCE_WINDOW_BYTES).min(source.len()));
    let half = start + (limit - start) / 2;
    source[start..limit]
        .rfind('\n')
        .map(|index| start + index + 1)
        .filter(|end| *end >= half && *end > start)
        .unwrap_or(limit)
}

fn floor_char_boundary(source: &str, mut offset: usize) -> usize {
    while !source.is_char_boundary(offset) {
        offset -= 1;
    }
    if offset > 0
        && offset < source.len()
        && source.as_bytes()[offset - 1] == b'\r'
        && source.as_bytes()[offset] == b'\n'
    {
        offset -= 1;
    }
    offset
}

fn next_char_end(source: &str, start: usize) -> usize {
    start + source[start..].chars().next().expect("start is before source end").len_utf8()
}

fn is_input_too_large(error: &SearchError) -> bool {
    error
        .embedding_failure()
        .is_some_and(|failure| failure.code == EmbeddingFailureCode::EmbeddingInputTooLarge)
}

fn invalid_config() -> SearchError {
    EmbeddingFailure::new(EmbeddingFailureCode::EmbeddingInvalidConfig).into()
}

fn input_too_large() -> SearchError {
    EmbeddingFailure::new(EmbeddingFailureCode::EmbeddingInputTooLarge).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_halving_covers_single_long_line_and_utf8() {
        let source = "é".repeat(267_706);
        assert_eq!(source.len(), 535_412);
        let mut calls = 0;
        let ranges = split_source(&source, &mut |_, slice| {
            calls += 1;
            if slice.len() > 4096 {
                Err(input_too_large())
            } else {
                Ok(())
            }
        })
        .unwrap();
        assert!(calls <= ranges.len() * 16);
        assert_eq!(ranges.first().unwrap().start, 0);
        assert_eq!(ranges.last().unwrap().end, source.len());
        assert!(ranges.iter().all(
            |range| source.is_char_boundary(range.start) && source.is_char_boundary(range.end)
        ));
        assert_eq!(ranges.iter().map(|range| &source[range.clone()]).collect::<String>(), source);

        let crlf = "строка\r\n".repeat(5000);
        let ranges = split_source(&crlf, &mut |_, slice| {
            if slice.len() > 300 {
                Err(input_too_large())
            } else {
                Ok(())
            }
        })
        .unwrap();
        assert!(ranges.iter().all(|range| {
            range.end == crlf.len()
                || !(range.end > 0
                    && crlf.as_bytes()[range.end - 1] == b'\r'
                    && crlf.as_bytes()[range.end] == b'\n')
        }));
    }

    #[test]
    fn oversized_mandatory_header_fails_without_source_progress() {
        let source = "body";
        let error = split_source(source, &mut |_, _| Err(input_too_large())).unwrap_err();
        assert!(is_input_too_large(&error));
    }

    #[test]
    fn crlf_scalar_can_fit_when_the_pair_does_not() {
        let source = "\r\n";
        let mut accepted = Vec::new();
        let ranges = split_source(source, &mut |_, slice| {
            if slice.len() > 1 {
                Err(input_too_large())
            } else {
                accepted.push(slice.to_owned());
                Ok(())
            }
        })
        .unwrap();
        assert_eq!(ranges, [0..1, 1..2]);
        assert_eq!(accepted, ["\r", "\n"]);
    }

    #[test]
    fn pinned_auto_tokenizer_parity_and_8192_boundary() {
        let Ok(path) = std::env::var("USER2_TOKENIZER_JSON") else { return };
        let bytes = std::fs::read(&path).unwrap();
        let actual =
            Sha256::digest(&bytes).iter().map(|byte| format!("{byte:02x}")).collect::<String>();
        assert_eq!(actual, "80d0433a2cfc55a4561b0e98b6f822decc48c9d457db498837223f9385ef3aff");
        let _ = Tokenizer::from_bytes(&bytes).unwrap();
        let policy = TokenPolicy::load(
            Path::new(&path),
            "80d0433a2cfc55a4561b0e98b6f822decc48c9d457db498837223f9385ef3aff",
            8192,
        )
        .unwrap();
        let parity = [
            (
                "Процедура Тест()\r\n    Возврат \"ёж\";\r\nКонецПроцедуры",
                vec![
                50281, 43565, 3008, 320, 43295, 208, 205, 2, 8957, 30574, 680, 862, 312, 30349,
                208, 205, 2443, 3285, 20548, 501, 444, 50282
                ],
            ),
            (
                "SELECT Номенклатура, Количество ИЗ Документ.РеализацияТоваровУслуг ГДЕ Проведен = ИСТИНА",
                vec![50281, 57, 7445, 26893, 1179, 45972, 2590, 18, 22498, 49397, 44255, 20, 30392, 8594, 6415, 323, 8988, 103, 35071, 866, 10903, 1878, 14987, 6440, 706, 21241, 8542, 50282],
            ),
            ("НайтиДокументПоНомеру", vec![50281, 1391, 299, 1490, 249, 14305, 698, 271, 260, 147, 129, 2834, 37403, 50282]),
            ("Что такое Ёж?", vec![50281, 3313, 3868, 47220, 312, 37, 50282]),
            ("[CLS] русский запрос [SEP]", vec![50281, 50281, 14521, 21304, 227, 50282, 50282]),
            ("Procedure Function If Then End", vec![50281, 8521, 2429, 975, 646, 15491, 2566, 8493, 17123, 50282]),
        ];
        for (input, expected_ids) in parity {
            let ids = policy.encode(input).unwrap();
            assert_eq!(ids, expected_ids, "tokenizer parity for {input:?}");
            assert_eq!(policy.count(input).unwrap(), ids.len());
        }
        assert_eq!(policy.count("").unwrap(), 2, "special tokens are included");
        assert!(policy.check(&"x".repeat(32_755)).is_ok());
        assert!(is_input_too_large(&policy.check(&"x".repeat(32_759)).unwrap_err()));
    }
}
