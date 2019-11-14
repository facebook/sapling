/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Utility functions

/// Expand csh style brace expressions (`{` `}`) used in a glob pattern.
/// Return multiple glob patterns. If the brackets do not match, return
/// an empty vector.
///
/// TODO: Fix error handling so it returns an error when the input is
/// illegal.
///
/// Examples:
///
/// ```
/// use pathmatcher::expand_curly_brackets;
///
/// assert_eq!(expand_curly_brackets("foo"), vec!["foo"]);
/// assert_eq!(expand_curly_brackets("foo{a,b,}"), vec!["fooa", "foob", "foo"]);
/// assert_eq!(expand_curly_brackets("a{b,c{d,e}f}g"), vec!["abg", "acdfg", "acefg"]);
/// assert_eq!(expand_curly_brackets("{a,b}{}{c,d}{{e}}"), vec!["ace", "ade", "bce", "bde"]);
/// assert_eq!(expand_curly_brackets("\\{a\\}"), vec!["\\{a\\}"]);
/// assert_eq!(expand_curly_brackets("[{a}]"), vec!["[{a}]"]);
/// assert!(expand_curly_brackets("a}").is_empty());
/// assert!(expand_curly_brackets("{a").is_empty());
/// ```
pub fn expand_curly_brackets(pat: &str) -> Vec<String> {
    // A DAG of string segments. Vec indexes are used as identities.
    #[derive(Default, Debug)]
    struct StrNode(String, Vec<usize>);
    let mut dag = vec![StrNode::default()];

    // Convert the pattern to a DAG. For example, "a{b,c{d,e}f}g" is
    // converted to:
    //   dag[0] = ("a", [1, 2])
    //   dag[1] = ("b", [6])
    //   dag[2] = ("c", [3, 4])
    //   dag[3] = ("d", [5])
    //   dag[4] = ("e", [5])
    //   dag[5] = ("f", [6])
    //   dag[6] = ("g", [])

    let mut in_box_brackets = false;
    let mut escaped = false;

    // "Current" StrNode id used before "{"
    let mut bracket_stack: Vec<usize> = Vec::new();

    for ch in pat.chars() {
        let mut need_write = true;
        if escaped {
            match ch {
                _ => escaped = false,
            }
        } else if in_box_brackets {
            match ch {
                ']' => in_box_brackets = false,
                _ => (),
            }
        } else {
            match ch {
                '\\' => escaped = true,
                '[' => in_box_brackets = true,
                '{' => {
                    let next_id = dag.len();
                    let current_id = next_id - 1;
                    dag.push(StrNode::default());
                    bracket_stack.push(current_id);
                    dag[current_id].1.push(next_id);
                    need_write = false;
                }
                '}' => {
                    if bracket_stack.is_empty() {
                        // ill-formed pattern - '}' without '{'
                        return Vec::new();
                    }
                    // "Merge" all "heads" in "{ ... }" into one node
                    let next_id = dag.len();
                    dag.push(StrNode::default());
                    let last_id = bracket_stack.pop().unwrap();
                    for id in last_id + 1..next_id {
                        let is_head = dag[id].1.is_empty();
                        if is_head {
                            dag[id].1.push(next_id);
                        }
                    }
                    need_write = false;
                }
                ',' if !bracket_stack.is_empty() => {
                    // Start another "head"
                    let next_id = dag.len();
                    let last_id: usize = *bracket_stack.last().unwrap();
                    dag[last_id].1.push(next_id);
                    dag.push(StrNode::default());
                    need_write = false;
                }
                _ => (),
            }
        }

        // Write to the "current" node. It's always the last one.
        if need_write {
            dag.last_mut().unwrap().0.push(ch);
        }
    }

    if !bracket_stack.is_empty() {
        // '{' and '}' mismatched
        return Vec::new();
    }

    // Traverse the DAG to get all expanded strings
    let mut result = Vec::new();
    fn visit(dag: &Vec<StrNode>, result: &mut Vec<String>, prefix: String, id: usize) {
        let prefix = prefix + &dag[id].0;
        if id == dag.len() - 1 {
            assert!(dag[id].1.is_empty());
            result.push(prefix);
        } else {
            for child_id in dag[id].1.iter().cloned() {
                visit(dag, result, prefix.clone(), child_id);
            }
        }
    }
    visit(&dag, &mut result, String::new(), 0);
    result
}

/// Normalize a less strict glob pattern to a strict glob pattern.
///
/// In a strict glob pattern, `**` can only be a single directory component.
pub fn normalize_glob(pat: &str) -> String {
    let mut result = String::with_capacity(pat.len());
    let chars: Vec<_> = pat.chars().collect();
    for (i, &ch) in chars.iter().enumerate() {
        if ch == '*'
            && chars.get(i + 1).cloned() == Some('*')
            && !result.ends_with('\\')
            && !result.is_empty()
            && !result.ends_with('/')
        {
            // Change 'a**' to 'a*/**'
            result += "*/";
        }
        result.push(ch);
        if ch == '*'
            && i > 0
            && chars[i - 1] == '*'
            && chars.get(i + 1) != None
            && chars.get(i + 1).cloned() != Some('/')
        {
            // Change '**a' to '**/*a'
            result += "/*";
        }
    }
    result
}

/// Escape special characters in a plain pattern so it can be used
/// as a glob pattern.
pub fn plain_to_glob(plain: &str) -> String {
    let mut result = String::with_capacity(plain.len());
    if plain.starts_with("!") {
        result.push('\\');
    }
    for ch in plain.chars() {
        match ch {
            '\\' | '*' | '{' | '}' | '[' | ']' => result.push('\\'),
            _ => (),
        }
        result.push(ch);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_glob() {
        assert_eq!(normalize_glob("**"), "**");
        assert_eq!(normalize_glob("a/**"), "a/**");
        assert_eq!(normalize_glob("a/b**"), "a/b*/**");
        assert_eq!(normalize_glob("a/b\\**"), "a/b\\**");
        assert_eq!(normalize_glob("a/**/c"), "a/**/c");
        assert_eq!(normalize_glob("a/**c"), "a/**/*c");
    }

    #[test]
    fn test_plain_to_glob() {
        assert_eq!(plain_to_glob("a[b{c*d\\e}]"), "a\\[b\\{c\\*d\\\\e\\}\\]");
        assert_eq!(plain_to_glob(""), "");
        assert_eq!(plain_to_glob("!a!"), "\\!a!");
    }
}
