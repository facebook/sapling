/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! # drawdag
//!
//! Utilities to parse ASCII revision DAG and create commits from them.

use std::collections::{BTreeMap, BTreeSet, HashSet};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Direction {
    /// From bottom to top. Roots are at the bottom.
    BottomTop,

    /// From left to right. Roots are at the left.
    LeftRight,
}

/// Parse an ASCII DAG. Extract edge information.
/// Return a map from names to their parents.
///
/// The direction of the graph is automatically detected.
/// If `|` is used, then roots are at the bottom, heads are at the top side.
/// Otherwise, `-` can be used, and roots are at the left, heads are at the
/// right. `|` and `-` cannot be used together.
///
/// # Example:
///
/// ```
/// use drawdag::parse;
///
/// let edges = parse(r#"
///             E
///              \
///     C----B----A
///        /
///      D-
/// "#);
/// let expected = "{\"A\": {\"B\", \"E\"}, \"B\": {\"C\", \"D\"}, \"C\": {}, \"D\": {}, \"E\": {}}";
/// assert_eq!(format!("{:?}", edges), expected);
///
/// let edges = parse(r#"
///   A
///  /|
/// | B
/// E |
///   |\
///   C D
/// "#);
/// assert_eq!(format!("{:?}", edges), expected);
/// ```
pub fn parse(text: impl AsRef<str>) -> BTreeMap<String, BTreeSet<String>> {
    use Direction::{BottomTop, LeftRight};

    // Detect direction.
    let direction = if text.as_ref().contains('|') {
        BottomTop
    } else {
        LeftRight
    };
    let lines: Vec<Vec<char>> = text
        .as_ref()
        .lines()
        .map(|line| line.chars().collect())
        .collect();

    // (y, x) -> char. Return a space if (y, x) is out of range.
    let get = |y: isize, x: isize| -> char {
        if y < 0 || x < 0 {
            ' '
        } else {
            lines
                .get(y as usize)
                .cloned()
                .map(|line| line.get(x as usize).cloned().unwrap_or(' '))
                .unwrap_or(' ')
        }
    };

    // Like `get`, but concatenate left and right parts if they look like a word.
    let get_name = |y: isize, x: isize| -> String {
        (0..x)
            .rev()
            .map(|x| get(y, x))
            .take_while(|&ch| is_name(ch))
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .chain((x..).map(|x| get(y, x)).take_while(|&ch| is_name(ch)))
            .collect()
    };

    // Follow the ASCII edges at the given position.
    let get_parents = |y: isize, x: isize| -> Vec<String> {
        let mut parents = Vec::new();
        let mut visited = HashSet::new();
        let mut visit = |y, x, expected: &'static str, to_visit: &mut Vec<(isize, isize, &str)>| {
            if !visited.contains(&(y, x, expected)) {
                visited.insert((y, x, expected));
                let ch = get(y, x);
                if is_name(ch) && expected.contains('t') {
                    // t: text
                    parents.push(get_name(y, x));
                    return;
                }
                if !expected.contains(ch) {
                    return;
                }
                match (ch, direction) {
                    (' ', _) => (),
                    ('|', BottomTop) => {
                        to_visit.push((y + 1, x - 1, "/"));
                        to_visit.push((y + 1, x, "|/\\t"));
                        to_visit.push((y + 1, x + 1, "\\"));
                    }
                    ('\\', BottomTop) => {
                        to_visit.push((y + 1, x + 1, "|\\t"));
                        to_visit.push((y + 1, x, "|t"));
                    }
                    ('/', BottomTop) => {
                        to_visit.push((y + 1, x - 1, "|/t"));
                        to_visit.push((y + 1, x, "|t"));
                    }
                    ('-', LeftRight) => {
                        to_visit.push((y - 1, x - 1, "\\"));
                        to_visit.push((y, x - 1, "-/\\t"));
                        to_visit.push((y + 1, x - 1, "/"));
                    }
                    ('\\', LeftRight) => {
                        to_visit.push((y - 1, x - 1, "-\\t"));
                        to_visit.push((y, x - 1, "-t"));
                    }
                    ('/', LeftRight) => {
                        to_visit.push((y + 1, x - 1, "-/t"));
                        to_visit.push((y, x - 1, "-t"));
                    }
                    _ => unreachable!(),
                }
            }
        };

        let mut to_visit: Vec<(isize, isize, &str)> = match direction {
            BottomTop => [(y + 1, x - 1, "/"), (y + 1, x, "|"), (y + 1, x + 1, "\\")],
            LeftRight => [(y - 1, x - 1, "\\"), (y, x - 1, "-"), (y + 1, x - 1, "/")],
        }
        .iter()
        .cloned()
        .filter(|(y, x, expected)| expected.contains(get(*y, *x)))
        .collect();
        while let Some((y, x, expected)) = to_visit.pop() {
            visit(y, x, expected, &mut to_visit);
        }

        parents
    };

    // Scan every character
    let mut edges: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for y in 0..lines.len() as isize {
        for x in 0..lines[y as usize].len() as isize {
            let ch = get(y, x);
            if is_name(ch) {
                let name = get_name(y, x);
                edges.entry(name.clone()).or_default();
                for parent in get_parents(y, x) {
                    edges.get_mut(&name).unwrap().insert(parent);
                }
            }
            // Sanity check
            match (ch, direction) {
                ('-', BottomTop) => panic!("'-' is incompatible with BottomTop direction"),
                ('|', LeftRight) => panic!("'|' is incompatible with LeftRight direction"),
                _ => (),
            }
        }
    }

    edges
}

/// Commit the DAG by using the given commit function.
///
/// The commit function takes two arguments: Commit identity by the ASCII dag,
/// and parents defined by the commit function. The commit function returns the
/// identity of the committed change, and this function will use them as parents
/// passed into the future `commit_func` calls.
pub fn commit(
    dag: &BTreeMap<String, BTreeSet<String>>,
    mut commit_func: impl FnMut(String, Vec<Box<[u8]>>) -> Box<[u8]>,
) {
    let mut committed: BTreeMap<String, Box<[u8]>> = BTreeMap::new();

    while committed.len() < dag.len() {
        let mut made_progress = false;
        for (name, parents) in dag.iter() {
            if !committed.contains_key(name)
                && parents.iter().all(|name| committed.contains_key(name))
            {
                let parent_ids = parents.iter().map(|name| committed[name].clone()).collect();
                let new_id = commit_func(name.clone(), parent_ids);
                committed.insert(name.to_string(), new_id);
                made_progress = true;
            }
        }
        assert!(made_progress, "graph contains cycles");
    }
}

/// Parse the ASCII DAG and commit it. See [`parse`] and [`commit`] for details.
pub fn drawdag(
    text: impl AsRef<str>,
    commit_func: impl FnMut(String, Vec<Box<[u8]>>) -> Box<[u8]>,
) {
    commit(&parse(text), commit_func)
}

fn is_name(ch: char) -> bool {
    ch.is_alphanumeric()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct CommitLog {
        log: String,
    }

    impl CommitLog {
        fn new() -> Self {
            Self { log: String::new() }
        }

        fn commit(&mut self, name: String, parents: Vec<Box<[u8]>>) -> Box<[u8]> {
            let new_id = self.log.chars().filter(|&ch| ch == '\n').count();
            let parents_str: Vec<String> = parents
                .into_iter()
                .map(|p| String::from_utf8(p.into_vec()).unwrap())
                .collect();
            self.log += &format!(
                "{}: {{ parents: {:?}, name: {} }}\n",
                new_id, parents_str, name
            );
            format!("{}", new_id).as_bytes().to_vec().into_boxed_slice()
        }
    }

    fn assert_drawdag(text: &str, expected: &str) {
        let mut log = CommitLog::new();
        drawdag(text, |n, p| log.commit(n, p));
        assert_eq!(log.log, expected);
    }

    #[test]
    #[should_panic]
    fn test_drawdag_cycle1() {
        let mut log = CommitLog::new();
        drawdag("A-B B-A", |n, p| log.commit(n, p));
    }

    #[test]
    #[should_panic]
    fn test_drawdag_cycle2() {
        let mut log = CommitLog::new();
        drawdag("A-B-C-A", |n, p| log.commit(n, p));
    }

    #[test]
    fn test_drawdag() {
        assert_drawdag(
            "A-C-B",
            r#"0: { parents: [], name: A }
1: { parents: ["0"], name: C }
2: { parents: ["1"], name: B }
"#,
        );

        assert_drawdag(
            r#"
    C-D-\     /--I--J--\
A-B------E-F-G-H--------K--L"#,
            r#"0: { parents: [], name: A }
1: { parents: ["0"], name: B }
2: { parents: [], name: C }
3: { parents: ["2"], name: D }
4: { parents: ["1", "3"], name: E }
5: { parents: ["4"], name: F }
6: { parents: ["5"], name: G }
7: { parents: ["6"], name: H }
8: { parents: ["6"], name: I }
9: { parents: ["8"], name: J }
10: { parents: ["7", "9"], name: K }
11: { parents: ["10"], name: L }
"#,
        );

        assert_drawdag(
            r#"
      G
      |
I D C F
 \ \| |
  H B E
   \|/
    A
"#,
            r#"0: { parents: [], name: A }
1: { parents: ["0"], name: B }
2: { parents: ["1"], name: C }
3: { parents: ["1"], name: D }
4: { parents: ["0"], name: E }
5: { parents: ["4"], name: F }
6: { parents: ["5"], name: G }
7: { parents: ["0"], name: H }
8: { parents: ["7"], name: I }
"#,
        );

        assert_drawdag(
            r#"
    A
   /|\
  H B E
 / /| |
I D C F
      |
      G
"#,
            r#"0: { parents: [], name: C }
1: { parents: [], name: D }
2: { parents: [], name: G }
3: { parents: [], name: I }
4: { parents: ["0", "1"], name: B }
5: { parents: ["2"], name: F }
6: { parents: ["3"], name: H }
7: { parents: ["5"], name: E }
8: { parents: ["4", "7", "6"], name: A }
"#,
        );
    }
}
