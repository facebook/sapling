/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

pub static ALL_PATTERN_KINDS: &[&str] = &[
    "re",
    "glob",
    "path",
    "relglob",
    "relpath",
    "relre",
    "listfile",
    "listfile0",
    "set",
    "include",
    "subinclude",
    "rootfilesin",
];

pub fn split_pattern<'a>(pattern: &'a str, default_kind: &'a str) -> (&'a str, &'a str) {
    match pattern.split_once(':') {
        Some((k, p)) => {
            if ALL_PATTERN_KINDS.contains(&k) {
                (k, p)
            } else {
                (default_kind, pattern)
            }
        }
        None => (default_kind, pattern),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_pattern() {
        let v = split_pattern("re:a.*py", "glob");
        assert_eq!(v, ("re", "a.*py"));

        let v = split_pattern("badkind:a.*py", "glob");
        assert_eq!(v, ("glob", "badkind:a.*py"));

        let v = split_pattern("a.*py", "re");
        assert_eq!(v, ("re", "a.*py"));
    }
}
