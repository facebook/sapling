/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

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
