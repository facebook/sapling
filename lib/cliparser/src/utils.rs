// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.
use std::char;
use std::ops::Range;

pub fn get_prefix_bounds(prefix: impl AsRef<str>) -> Range<String> {
    let mut upper = prefix.as_ref().to_string();
    assert!(!upper.is_empty());

    let mut last_char = upper.pop().unwrap();
    let mut last_char_code: u32 = last_char as u32;

    while let Some(next_val) = last_char_code.checked_add(1) {
        if let Some(c) = char::from_u32(next_val) {
            last_char = c;
            break;
        }
        last_char_code = next_val;
    }

    upper.push(last_char);

    prefix.as_ref().to_string()..upper
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_prefix_bounds_one_letter() {
        let prefix = "a";
        let end = "b";
        let range = get_prefix_bounds(prefix);
        assert_eq!(range.start.as_str(), prefix);
        assert_eq!(range.end.as_str(), end);
    }

    #[test]
    fn test_get_prefix_bounds_ending_with_z() {
        let prefix = "z";
        let end = "{";
        let range = get_prefix_bounds(prefix);
        assert_eq!(range.start.as_str(), prefix);
        assert_eq!(range.end.as_str(), end);
    }

    #[test]
    fn test_get_prefix_bounds_multiple() {
        let prefix = "comm"; // prefix of commit
        let end = "comn";
        let range = get_prefix_bounds(prefix);
        assert_eq!(range.start.as_str(), prefix);
        assert_eq!(range.end.as_str(), end);
    }

    #[test]
    fn test_get_prefix_bounds_ending_space() {
        let prefix = "comm "; // prefix of commit with trailing space
        let end = "comm!";
        let range = get_prefix_bounds(prefix);
        assert_eq!(range.start.as_str(), prefix);
        assert_eq!(range.end.as_str(), end);
    }

    #[test]
    fn test_get_prefix_bounds_unicode() {
        let prefix = "\u{1F36A}"; // Cookie Emoji
        let end = "\u{1F36B}";
        let range = get_prefix_bounds(prefix);
        assert_eq!(range.start.as_str(), prefix);
        assert_eq!(range.end.as_str(), end);
    }
}
