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

/// Returns true if and only if we can get q by repeatedly applying str_succ to p.
pub(crate) fn is_successor(p: &str, q: &str) -> bool {
    // The first vector returned contains the characters of s
    // with the last contiguous substring of alphanumeric characters
    // replaced with 0, and the second vector contains the characters
    // of that replaced substring.
    fn get_skeleton(s: &str) -> (Vec<char>, Vec<char>) {
        let mut skeleton = Vec::new();
        let mut last_alphanumeric = Vec::new();
        let mut last_alphanumeric_complete = false;

        for c in s.chars().rev() {
            if last_alphanumeric_complete {
                skeleton.push(c);
            } else {
                match (
                    CharRange::from_char(c),
                    skeleton.last().and_then(|c| CharRange::from_char(*c)),
                ) {
                    (Some(_), None) => {
                        skeleton.push('0');
                        last_alphanumeric = vec![c];
                    }
                    (Some(_), Some(_)) => {
                        last_alphanumeric.push(c);
                    }
                    (None, Some(_)) => {
                        skeleton.push(c);
                        last_alphanumeric_complete = true;
                    }
                    (None, None) => {
                        skeleton.push(c);
                    }
                }
            }
        }

        skeleton.reverse();
        last_alphanumeric.reverse();

        // Append a zero to the end to make sure str_succ preserves the skeleton.
        if last_alphanumeric.is_empty() {
            skeleton.push('0');
        }

        (skeleton, last_alphanumeric)
    }

    // Split the given vector into two vectors, the prefix that
    // consists of characters that have the same CharRange and the remainder.
    fn split_once_by_char_range(alphanumeric: Vec<char>) -> (Vec<char>, Vec<char>) {
        let mut found_different = false;
        let mut prefix = Vec::new();
        let mut remaining = Vec::new();

        for c in alphanumeric {
            if found_different {
                remaining.push(c);
            } else {
                match (
                    CharRange::from_char(c),
                    prefix.last().and_then(|c| CharRange::from_char(*c)),
                ) {
                    (Some(_), None) => {
                        prefix.push(c);
                    }
                    (Some(a), Some(b)) if a == b => {
                        prefix.push(c);
                    }
                    _ => {
                        remaining.push(c);
                        found_different = true;
                    }
                }
            }
        }

        (prefix, remaining)
    }

    let (p_skeleton, mut p_last_alphanumeric) = get_skeleton(p);
    let (q_skeleton, q_last_alphanumeric) = get_skeleton(q);

    // str_succ will never change the skeleton.
    if p_skeleton != q_skeleton {
        return false;
    }

    // Empty last_alphanumeric requires special handling.
    match (
        p_last_alphanumeric.is_empty(),
        q_last_alphanumeric.is_empty(),
    ) {
        (true, true) => {
            return true;
        }
        (true, false) => {
            p_last_alphanumeric.push('1');
        }
        (false, true) => {
            return false;
        }
        (false, false) => {}
    }

    // Whenever str_succ increases the length there can never be leading 0s.
    if q_last_alphanumeric.len() > p_last_alphanumeric.len()
        && q_last_alphanumeric.first() == Some(&'0')
    {
        return false;
    }

    let (p_prefix, p_remaining) = split_once_by_char_range(p_last_alphanumeric);
    let (q_prefix, q_remaining) = split_once_by_char_range(q_last_alphanumeric);

    // str_succ preserves the CharRange of the first character.
    if p_prefix.first().map(|c| CharRange::from_char(*c))
        != q_prefix.first().map(|c| CharRange::from_char(*c))
    {
        return false;
    }

    // The length and the CharRanges of the characters in the remaining part will never change.
    if p_remaining.len() != q_remaining.len()
        || p_remaining
            .iter()
            .zip(q_remaining.iter())
            .any(|(x, y)| CharRange::from_char(*x) != CharRange::from_char(*y))
    {
        return false;
    }

    // Check by length first then lexicographically.
    !(p_prefix.len() > q_prefix.len()
        || (p_prefix.len() == q_prefix.len() && p_prefix > q_prefix)
        || (p_prefix == q_prefix && p_remaining > q_remaining))
}

/// Range of an ASCII char.
#[derive(Copy, Clone, PartialEq)]
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
        Self::ALL.iter().copied().find(|range| {
            let (start, end) = range.bound();
            ch >= start && ch <= end
        })
    }
}

#[cfg(test)]
#[test]
fn test_str_succ() {
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

#[cfg(test)]
#[test]
fn test_is_successor() {
    assert!(is_successor("D", "E"));
    assert!(is_successor("AZ", "BC"));
    assert!(is_successor("ZZ", "AAA"));
    assert!(is_successor("A", "AA"));
    assert!(is_successor("d", "aa"));
    assert!(is_successor("d", "e"));
    assert!(is_successor("0", "99"));
    assert!(is_successor("11", "12"));
    assert!(is_successor("123", "9000"));
    assert!(is_successor("0123", "0900"));
    assert!(is_successor("z123", "bc904"));
    assert!(is_successor("a900", "b900"));
    assert!(is_successor("a(123)", "a(9000)"));
    assert!(is_successor("()", "()100"));
    assert!(is_successor("()", "()"));

    assert!(!is_successor("E", "D"));
    assert!(!is_successor("e", "d"));
    assert!(!is_successor("d", "E"));
    assert!(!is_successor("12", "11"));
    assert!(!is_successor("0", "099"));
    assert!(!is_successor("123", "0900"));
    assert!(!is_successor("a900", "a123"));
    assert!(!is_successor("b900", "a900"));
    assert!(!is_successor("a(123)", "b(9000)"));
    assert!(!is_successor("()", "()0"));
    assert!(!is_successor("()", "()A"));
    assert!(!is_successor("()", "()a"));
    assert!(!is_successor("()", "()0"));
    assert!(!is_successor("()1", "()"));
}
