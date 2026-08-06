/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use bitflags::bitflags;
use caseless::Caseless;
use unicode_normalization::UnicodeNormalization;

const NO_STATE: u8 = u8::MAX;

bitflags! {
    #[derive(Copy, Clone, Debug)]
    pub(crate) struct Normalization: u8 {
        /// Case normalization.
        /// When set without NFD: ASCII case folding.
        /// When set with NFD: Unicode case folding. e.g. `ſ` -> `s`.
        /// In Python, use `s.casefold()` to check.
        const CASE = 1;
        /// NFD normalization.
        /// e.g. `K` (`\u212a`) -> `K`, `;` (`\u037e`) -> `;`
        /// In Python, use `unicodedata.normalize('NFD', c)` to check.
        const NFD = 16;
    }
}

pub(crate) struct WordSet {
    transitions: Vec<[u8; 128]>,
    accepting: Vec<bool>,
}

impl WordSet {
    pub(crate) fn new(words: Vec<&'static str>) -> Self {
        let state_capacity = 1 + words.iter().map(|word| word.len()).sum::<usize>();
        let mut transitions = Vec::with_capacity(state_capacity);
        let mut accepting = Vec::with_capacity(state_capacity);
        transitions.push([NO_STATE; 128]);
        accepting.push(false);

        for word in words {
            // Case-insensitive check requires ASCII lowercase.
            assert_eq!(word, word.to_ascii_lowercase());
            // ASCII-only words allow fast path (skip slow NFD on ASCII-only input).
            assert!(word.is_ascii());

            let mut state = 0;
            for byte in word.bytes() {
                assert!(byte < 128);
                let next_state = transitions[state][byte as usize];
                state = if next_state == NO_STATE {
                    let next_state = transitions.len();
                    assert!(next_state < NO_STATE as usize);
                    transitions.push([NO_STATE; 128]);
                    accepting.push(false);
                    transitions[state][byte as usize] = next_state as u8;
                    next_state
                } else {
                    next_state as usize
                };
            }
            accepting[state] = true;
        }

        Self {
            transitions,
            accepting,
        }
    }

    pub(crate) fn contains(&self, word: &str, normalization: Normalization) -> bool {
        let iter = word.bytes();
        // Run ASCII-only check first. ASCII case folding is 4x faster than NFD folding.
        let mut res = if normalization.contains(Normalization::CASE) {
            self.contains_iter(iter.map(|b| b.to_ascii_lowercase()))
        } else {
            self.contains_iter(iter)
        };
        // Run NFD normalization only on non-ASCII `word`.
        if normalization.contains(Normalization::NFD) && res.is_none() {
            let to_bytes = |c: char| {
                let mut bytes = [0; char::MAX_LEN_UTF8];
                let len = c.encode_utf8(&mut bytes).len();
                bytes.into_iter().take(len)
            };
            res = if normalization.contains(Normalization::CASE) {
                self.contains_iter(
                    word.chars()
                        .nfd()
                        .default_case_fold()
                        .nfd()
                        .flat_map(to_bytes),
                )
            } else {
                self.contains_iter(word.chars().nfd().flat_map(to_bytes))
            };
        }
        res.unwrap_or(false)
    }

    /// Returns `None` if a non-ASCII byte is encountered before a mismatch.
    pub(crate) fn contains_iter(&self, word: impl IntoIterator<Item = u8>) -> Option<bool> {
        let mut state = 0;
        for byte in word.into_iter() {
            if byte > 127 {
                return None;
            }
            let next_state = self.transitions[state][byte as usize];
            if next_state == NO_STATE {
                return Some(false);
            }
            state = next_state as usize;
        }
        Some(self.accepting[state])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_word_set() {
        let words = WordSet::new(vec!["a", "abc", "bc"]);
        let none = Normalization::empty();
        assert!(words.contains("a", none));
        assert!(words.contains("abc", none));
        assert!(!words.contains("ab", none));
        assert!(!words.contains("ABC", none));
        assert!(words.contains("ABC", Normalization::CASE));
    }

    #[test]
    fn test_nfd_folding() {
        let words = WordSet::new(vec!["sl", "strasse", ";"]);

        // NFD and case folding
        let flags = Normalization::NFD | Normalization::CASE;
        assert!(words.contains("ſL", flags));
        assert!(!words.contains("Ｓl", flags)); // NFKD, not NFD
        assert!(words.contains("Straße", flags));

        // NFD without case folding
        let flags = Normalization::NFD;
        let u037e = ";";
        assert!(words.contains(u037e, flags));
        assert!(!words.contains(u037e, Normalization::empty()));
        assert!(!words.contains("ſl", flags)); // case folding, not NFD
    }

    #[test]
    fn test_state_size_good_for_main_usecase() {
        // Does not panic (too many states)
        let _w = WordSet::new(vec![".", "..", ".hg", ".sl", ".git", ".repo", ".jj"]);
    }
}
