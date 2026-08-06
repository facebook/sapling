/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

const NO_STATE: usize = usize::MAX;

pub(crate) struct WordSet {
    transitions: Vec<[usize; 256]>,
    accepting: Vec<bool>,
}

impl WordSet {
    pub(crate) fn new(words: Vec<&'static str>) -> Self {
        let state_capacity = 1 + words.iter().map(|word| word.len()).sum::<usize>();
        let mut transitions = Vec::with_capacity(state_capacity);
        let mut accepting = Vec::with_capacity(state_capacity);
        transitions.push([NO_STATE; 256]);
        accepting.push(false);

        for word in words {
            // Case-insensitive check requires ASCII lowercase.
            assert_eq!(word, word.to_ascii_lowercase());

            let mut state = 0;
            for byte in word.bytes() {
                let next_state = transitions[state][byte as usize];
                state = if next_state == NO_STATE {
                    let next_state = transitions.len();
                    transitions.push([NO_STATE; 256]);
                    accepting.push(false);
                    transitions[state][byte as usize] = next_state;
                    next_state
                } else {
                    next_state
                };
            }
            accepting[state] = true;
        }

        Self {
            transitions,
            accepting,
        }
    }

    pub(crate) fn contains(&self, word: &str, case_insensitive: bool) -> bool {
        let iter = word.bytes();
        if case_insensitive {
            self.contains_iter(iter.map(|b| b.to_ascii_lowercase()))
        } else {
            self.contains_iter(iter)
        }
    }

    pub(crate) fn contains_iter(&self, word: impl IntoIterator<Item = u8>) -> bool {
        let mut state = 0;
        for byte in word.into_iter() {
            state = self.transitions[state][byte as usize];
            if state == NO_STATE {
                return false;
            }
        }
        self.accepting[state]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_word_set() {
        let words = WordSet::new(vec!["a", "abc", "bc"]);

        assert!(words.contains("a", false));
        assert!(words.contains("abc", false));
        assert!(!words.contains("ab", false));
        assert!(!words.contains("ABC", false));
        assert!(words.contains("ABC", true));
    }
}
