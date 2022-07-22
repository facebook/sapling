/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/// String successor by incrementing characters.
/// Similar to Ruby's `String#succ` [1], except if all characters are
/// non-alphanumeric, this function appends `1` to the end.
///
/// [1]: https://ruby-doc.org/core-3.1.0/String.html#method-i-next
pub(crate) fn str_succ(s: &str) -> String {
    let mut chars: Vec<char> = s.chars().collect();

    // The first character to be incremented is the rightmost alphanumeric.
    let index: Option<usize> = chars
        .iter()
        .enumerate()
        .rev()
        .filter_map(|(i, c)| CharRange::from_char(*c).map(|_| i))
        .next();
    match index {
        Some(index) => {
            let mut carry: char = '1';
            for i in (0..=index).rev() {
                let ch = chars[i];
                let range = match CharRange::from_char(ch) {
                    None => {
                        chars.insert(i + 1, carry);
                        break;
                    }
                    Some(range) => range,
                };
                carry = range.carry();
                let (start, end) = range.bound();
                if ch == end {
                    chars[i] = start;
                    if i == 0 {
                        chars.insert(0, range.carry());
                    }
                } else {
                    chars[i] = ((ch as u8) + 1) as char;
                    break;
                }
            }
            chars.into_iter().collect()
        }
        None => format!("{}1", s),
    }
}

/// Range of an ASCII char.
#[derive(Copy, Clone)]
enum CharRange {
    Digit,
    LowerLetter,
    UpperLetter,
}

impl CharRange {
    const ALL: &'static [Self] = &[Self::Digit, Self::LowerLetter, Self::UpperLetter];

    /// Get the start and end char in the range.
    fn bound(self) -> (char, char) {
        match self {
            CharRange::Digit => ('0', '9'),
            CharRange::LowerLetter => ('a', 'z'),
            CharRange::UpperLetter => ('A', 'Z'),
        }
    }

    /// Get the char used when carrying to a new char.
    fn carry(self) -> char {
        match self {
            CharRange::Digit => '1',
            CharRange::LowerLetter => 'a',
            CharRange::UpperLetter => 'A',
        }
    }

    /// Convert a char to `CharRange` if it's in a known range.
    fn from_char(ch: char) -> Option<CharRange> {
        Self::ALL
            .iter()
            .copied()
            .filter(|range| {
                let (start, end) = range.bound();
                ch >= start && ch <= end
            })
            .next()
    }
}

#[cfg(test)]
#[test]
fn test_str_succ_digits() {
    // Alphanumeric cases are consistent with Ruby.
    assert_eq!(str_succ("0"), "1");
    assert_eq!(str_succ("1"), "2");
    assert_eq!(str_succ("9"), "10");
    assert_eq!(str_succ("233"), "234");
    assert_eq!(str_succ("999"), "1000");
    assert_eq!(str_succ("0099"), "0100");
    assert_eq!(str_succ("1180591620717411303424"), "1180591620717411303425");

    assert_eq!(str_succ("a-99"), "a-100");
    assert_eq!(str_succ("aa99"), "ab00");
    assert_eq!(str_succ("aa99.."), "ab00..");

    assert_eq!(str_succ("a"), "b");
    assert_eq!(str_succ("C"), "D");
    assert_eq!(str_succ("ASDF"), "ASDG");
    assert_eq!(str_succ("zz"), "aaa");

    assert_eq!(str_succ("Zz"), "AAa");
    assert_eq!(str_succ("9z9Z"), "10a0A");

    // Non-alphanumeric cases are different from Ruby.
    assert_eq!(str_succ(""), "1");
    assert_eq!(str_succ("."), ".1");
}
