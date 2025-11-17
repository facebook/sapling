/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::sync::LazyLock;

use minibytes::Bytes;

/// Generate `Bytes` with given length. Useful for dummy file contents.
pub fn generate_file_content_of_length(len: usize) -> Bytes {
    const LOREM_IPSUM: &str = r#"Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do
eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad
minim veniam, quis nostrud exercitation ullamco laboris nisi ut
aliquip ex ea commodo consequat. Duis aute irure dolor in
reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla
pariatur. Excepteur sint occaecat cupidatat non proident, sunt in
culpa qui officia deserunt mollit anim id est laborum.

"#;
    // Avoid allocation for small blobs.
    static LOREM_IPSUM_LONG: LazyLock<Bytes> = LazyLock::new(|| {
        const SIZE: usize = 4_000_000;
        let long_text = LOREM_IPSUM.repeat(SIZE / LOREM_IPSUM.len() + 1);
        Vec::from(long_text).into()
    });
    if len <= LOREM_IPSUM_LONG.len() {
        LOREM_IPSUM_LONG.slice(..len)
    } else {
        let mut result = Vec::with_capacity(len);
        while result.len() < len {
            let wanted_len = (len - result.len()).min(LOREM_IPSUM_LONG.len());
            result.extend_from_slice(&LOREM_IPSUM_LONG[..wanted_len]);
        }
        result.into()
    }
}

/// Generate `String` with given length. Useful for slightly more interesting file contents.
pub fn generate_paragraphs(len: usize, seed: u64) -> String {
    let mut output = String::with_capacity(len);
    TrigramTextGen::default()
        .with_seed(seed)
        .generate_paragraphs(len, &mut output);
    assert_eq!(output.len(), len);
    output
}

static CORPUS: LazyLock<String> = LazyLock::new(|| {
    let compressed = include_bytes!("corpus.zst");
    let decompressed = zstdelta::apply(b"", compressed).unwrap();
    let text = String::from_utf8(decompressed).unwrap();
    debug_assert!(
        text.is_ascii(),
        "corpus must be ascii to be split at any position"
    );
    text
});
/// Note: some words will end with '\n'. They indidate the end of a paragraph.
static WORDS: LazyLock<Vec<&'static str>> = LazyLock::new(|| CORPUS.split(' ').collect());

// Given 2 words, predicate the next word.
static PREDICATION: LazyLock<HashMap<(&'static str, &'static str), Vec<&'static str>>> =
    LazyLock::new(|| {
        let mut map = HashMap::<_, Vec<&str>>::with_capacity(14777);
        for i in 0..(WORDS.len() - 2) {
            map.entry((WORDS[i], WORDS[i + 1]))
                .or_default()
                .push(WORDS[i + 2]);
        }
        map
    });

// Used to pick a "starting" point (two words).
static STARTING_WORDS: LazyLock<Vec<(&'static str, &'static str)>> = LazyLock::new(|| {
    let mut result: Vec<_> = PREDICATION
        .keys()
        .filter(|(k1, k2)| {
            k1.chars().next().unwrap_or(' ').is_ascii_uppercase()
                && !k1.ends_with('.')
                && !k2.ends_with('.')
        })
        .copied()
        .collect();
    result.sort_unstable();
    result
});

const MAX_LINE_WIDTH: usize = 76;

/// Handles line wrap, and word generation.
#[derive(Default)]
pub(crate) struct TrigramTextGen {
    seed: u64,
    // TextGen state.
    words: (&'static str, &'static str),
    line_width: usize,
}

impl TrigramTextGen {
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }

    /// Generate (multiple) paragraphs to `output`.
    /// `output` length should be `len`.
    pub fn generate_paragraphs(&mut self, len: usize, output: &mut String) {
        while output.len() < len {
            self.generate_paragraph(len, output);
        }
        assert_eq!(output.len(), len);
    }

    /// Generate a paragraph. If `output` is near the `len` length, try to end
    /// the paragraph.
    pub fn generate_paragraph(&mut self, len: usize, output: &mut String) {
        self.words = ("", "");
        self.line_width = 0;
        while output.len() < len {
            self.generate_word(output);
            // Too long? Trunate it.
            if output.len() > len {
                output.truncate(len);
                break;
            }
            // End of paragraph?
            if output.ends_with('\n') {
                break;
            }
        }
    }

    /// Generate the next word. Make `output` longer.
    /// `output` should not exceed `len`.
    fn generate_word(&mut self, output: &mut String) {
        match PREDICATION.get(&self.words) {
            Some(choices) => {
                let word = *self.sample(choices);
                self.push_word(word, output);
                self.words = if word.ends_with('\n') {
                    ("", "")
                } else {
                    (self.words.1, word)
                };
            }
            None => {
                self.words = *self.sample(&*STARTING_WORDS);
                self.push_word(self.words.0, output);
                self.push_word(self.words.1, output);
            }
        }
    }

    fn push_word(&mut self, mut word: &str, output: &mut String) {
        debug_assert!(!word.is_empty());
        if self.line_width + word.len() + 1 > MAX_LINE_WIDTH {
            output.push('\n');
            self.line_width = 0;
            // Do not wrap line immediately.
            word = word.trim_ascii_end();
        } else {
            if !output.is_empty() {
                let separator = if output.ends_with('\n') { '\n' } else { ' ' };
                output.push(separator);
            }
            self.line_width += 1;
        }
        output.push_str(word);
        self.line_width += word.len();
        if word.ends_with('\n') {
            self.line_width = 0;
        }
    }

    fn sample<T>(&mut self, items: &'static [T]) -> &'static T {
        let index = split_mix64(&mut self.seed);
        sample(items, index)
    }
}

fn sample<T>(items: &'static [T], index: u64) -> &'static T {
    let len = items.len();
    match len {
        1 => &items[0],
        2 => &items[(index as usize) & 0b1],
        _ => &items[(index as usize) % len],
    }
}

// https://rosettacode.org/wiki/Pseudo-random_numbers/Splitmix64
fn split_mix64(x: &mut u64) -> u64 {
    *x = x.wrapping_add(0x9e3779b97f4a7c15);
    let mut z = *x;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
    z ^ (z >> 31)
}

/// Generate a file name. For a same `seed`, different `id`s should generate
/// different names.
/// The generated file name has O(id) length. Practically, keep `id` relatively
/// small to avoid excessively long name.
pub fn generate_file_name(id: u64, seed: u64) -> String {
    // For each 4-bit of id, generate a word based on related 2-bit seed.
    let len = visit_names(id, seed, 0, |sum, word| sum + word.len() + 1);
    let name = String::with_capacity(len - 1);
    let name = visit_names(id, seed, name, |mut name, word| {
        if !name.is_empty() {
            name.push('-');
        }
        name.push_str(word);
        name
    });
    name
}

static NAMES: [[&str; 16]; 4] = [
    // Alphabet
    [
        "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p",
    ],
    // Roman
    [
        "I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X", "XI", "XII", "XIII", "XIV",
        "XV", "XVI",
    ],
    // Color
    [
        "red", "blue", "green", "yellow", "orange", "purple", "pink", "brown", "black", "white",
        "gray", "cyan", "magenta", "teal", "indigo", "violet",
    ],
    // Fruit
    [
        "apple", "orange", "lemon", "berry", "kiwi", "grape", "mango", "peach", "cherry", "melon",
        "pear", "plum", "lime", "banana", "coconut", "papaya",
    ],
];

/// Used by `generate_file_name`.
fn visit_names<T>(mut n: u64, mut choice: u64, init: T, f: impl Fn(T, &'static str) -> T) -> T {
    let mut value = init;
    let mut first = true;
    while first || n > 0 {
        let names = &NAMES[(choice & 0b11) as usize];
        value = f(value, names[(n & 15) as usize]);
        n >>= 4;
        choice >>= 2;
        first = false;
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_content_matches_requested_length() {
        for len in [
            0, 1, 2, 10, 50, 100, 500, 1000, 2000, 5000, 3_999_999, 4_000_000, 4_000_001,
            16_000_000, 20_000_000,
        ] {
            let blob = generate_file_content_of_length(len);
            assert_eq!(blob.len(), len);
        }
    }

    #[test]
    fn test_generate_paragraphs() {
        // Same seed share the same prefix.
        assert_eq!(
            generate_paragraphs(40, 42),
            "King: however, it only grinned when it g"
        );
        assert_eq!(
            generate_paragraphs(100, 42),
            r#"King: however, it only grinned when it grunted again, so she turned away.

Beautiful, beautiful Soup"#
        );
        assert_eq!(
            generate_paragraphs(200, 42),
            r#"King: however, it only grinned when it grunted again, so she turned away.

Beautiful, beautiful Soup!

Five and Seven said nothing, but looked at Alice, and her eyes filled with
tears running down his"#
        );
        // Different seed produces different text.
        assert_eq!(
            generate_paragraphs(550, 44),
            r#"He says it kills all the time it all is! I'll try the whole party swam to
the game.

Ada, she said, for her neck from being broken. She hastily put down the
hall. Queen turned angrily away from him, and said anxiously to herself,
Now, what am I to get her head to feel very sleepy and stupid), whether the
pleasure of making a daisy-chain would be four thousand miles down, I think-
(for, you see, so many lessons to learn! No, I've made up my mind about it;
and while she was now the right way of settling all difficulties, great or
small. Off with "#
        );
    }

    #[test]
    fn test_names_unique() {
        use std::collections::HashSet;
        for (i, names) in NAMES.iter().enumerate() {
            let set: HashSet<_> = names.iter().collect();
            assert_eq!(set.len(), names.len(), "NAMES[{i}] has duplicated items");
        }
    }

    #[test]
    fn test_file_name_examples() {
        assert_eq!(
            generate_file_name(0xabcd, 0b00_01_10_11),
            "banana-magenta-XII-k"
        );

        let g = |start: u64, end: u64, seed| -> Vec<String> {
            (start..=end).map(|i| generate_file_name(i, seed)).collect()
        };
        assert_eq!(
            g(0, 20, 0),
            [
                "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p",
                "a-b", "b-b", "c-b", "d-b", "e-b"
            ]
        );
        assert_eq!(
            g(0, 20, 0b1110),
            [
                "red",
                "blue",
                "green",
                "yellow",
                "orange",
                "purple",
                "pink",
                "brown",
                "black",
                "white",
                "gray",
                "cyan",
                "magenta",
                "teal",
                "indigo",
                "violet",
                "red-orange",
                "blue-orange",
                "green-orange",
                "yellow-orange",
                "orange-orange"
            ]
        );
    }
}
