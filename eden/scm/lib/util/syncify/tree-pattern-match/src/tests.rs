/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

pub use crate::find_all;
pub use crate::replace_all;
pub use crate::Match;

type Item = crate::Item<String>;

macro_rules! parse_item {
    ($v:literal) => { from_str($v) };
    ($v:ident) => { from_str(stringify!($v)) };
    (( $( $arg:tt )* )) => {{
        let args = vec![ $( parse_item!($arg), )* ];
        Item::Tree("()".to_owned(), args)
    }};
    ([ $( $arg:tt )* ]) => {{
        let args = vec![ $( parse_item!($arg), )* ];
        Item::Tree("[]".to_owned(), args)
    }};
    ({ $( $arg:tt )* }) => {{
        let args = vec![ $( parse_item!($arg), )* ];
        Item::Tree("{}".to_owned(), args)
    }};
}

macro_rules! parse {
    ( $( $arg:tt )* ) => {{
        let args: Vec<Item> = vec![ $( parse_item!($arg), )* ];
        args
    }};
}

fn unparse(items: &[Item]) -> String {
    let mut out: Vec<String> = Vec::new();
    for item in items {
        match item {
            Item::Tree(s, t) => {
                out.push(s[0..1].to_owned());
                out.push(unparse(t));
                out.push(s[1..2].to_owned());
            }
            Item::Item(s) => out.push(s.clone()),
            Item::Placeholder(s) => out.push(s.clone()),
        }
    }
    out.join(" ")
}

fn from_str(s: &str) -> Item {
    if s.starts_with("__") {
        Item::Placeholder(s.to_owned())
    } else {
        Item::Item(s.to_owned())
    }
}

impl Match<String> {
    fn show(&self) -> Vec<String> {
        let mut captures: Vec<_> = self.captures.iter().collect();
        captures.sort_unstable_by_key(|(p, _)| p.to_owned());
        captures
            .into_iter()
            .map(|(placeholder, items)| format!("{} => {}", placeholder, unparse(items)))
            .collect()
    }
}

#[test]
fn test_find_all_multiple_matches() {
    let items = parse!(a b a c);
    let pat = parse!(a __1);
    let matches = find_all(&items, &pat);
    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0].show(), ["__1 => b"]);
    assert_eq!(matches[1].show(), ["__1 => c"]);
}

#[test]
fn test_find_all_multiple_captures() {
    let items = parse!(async fn foo (a b c d) a d);
    let pat = parse!(fn __1 (a ___2 d) __3 d);
    let matches = find_all(&items, &pat);
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].show(), ["__1 => foo", "__3 => a", "___2 => b c"]);
}

#[test]
fn test_find_all_empty_captures() {
    let items = parse!(a b c []);
    let pat = parse!(a b ___1 c);
    let matches = find_all(&items, &pat);
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].show(), ["___1 => "]);
    let pat = parse!([___1]);
    let matches = find_all(&items, &pat);
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].show(), ["___1 => "]);
}

#[test]
fn test_find_all_nested_captures() {
    let items = parse!(attributes [ [ a b ] to [ c d ] and [ e ] to [ f ] ]);
    let pat = parse!([ __1 ] to [ __2 ]);
    let matches = find_all(&items, &pat);
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].show(), ["__1 => e", "__2 => f"]);

    let pat = parse!([ ___1 ] to [ ___2 ]);
    let matches = find_all(&items, &pat);
    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0].show(), ["___1 => a b", "___2 => c d"]);
}

#[test]
fn test_replace_all() {
    let items = parse!(async fn foo [B is Future [T] Send] (a b) Future [ R ] { a dot await and b dot await });
    let items = replace_all(items, &parse!(dot await), &parse!());
    assert_eq!(
        unparse(&items),
        "async fn foo [ B is Future [ T ] Send ] ( a b ) Future [ R ] { a and b }"
    );
    let items = replace_all(items, &parse!([ B ___g1 Send ]), &parse!());
    assert_eq!(
        unparse(&items),
        "async fn foo ( a b ) Future [ R ] { a and b }"
    );
    let items = replace_all(items, &parse!(Future[__1]), &parse!(__1));
    assert_eq!(unparse(&items), "async fn foo ( a b ) R { a and b }");
    let items = replace_all(items, &parse!(a ___1 b), &parse!(b ___1 a));
    assert_eq!(unparse(&items), "async fn foo ( b a ) R { b and a }");
}
