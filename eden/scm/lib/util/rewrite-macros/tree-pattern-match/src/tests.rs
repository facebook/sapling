/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::find_all;
use crate::matches_full;
use crate::matches_start;
use crate::replace_all;
use crate::tree_match::PlaceholderExt;
use crate::Match;
use crate::Placeholder;

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
            Item::Placeholder(p) => out.push(p.name().to_string()),
        }
    }
    out.join(" ")
}

fn from_str(s: &str) -> Item {
    if s.starts_with("__") {
        Item::Placeholder(Placeholder::new(s.to_owned()))
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
fn test_simple_matches() {
    let items = parse!(a b c);
    let pat = parse!(a __1 c);
    let matches = find_all(&items, &pat);
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].show(), ["__1 => b"]);

    let pat = parse!(a ___1 c);
    let matches = find_all(&items, &pat);
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].show(), ["___1 => b"]);

    let pat = parse!(a b ___1 c);
    let matches = find_all(&items, &pat);
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].show(), ["___1 => "]);

    let pat = parse!(___1);
    let matches = find_all(&items, &pat);
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].show(), ["___1 => a b c"]);

    let pat = parse!(__1 __2 __3);
    let matches = find_all(&items, &pat);
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].show(), ["__1 => a", "__2 => b", "__3 => c"]);

    let pat = parse!(__1 __2);
    let matches = find_all(&items, &pat);
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].show(), ["__1 => a", "__2 => b"]);

    let pat = parse!(__1);
    let matches = find_all(&items, &pat);
    assert_eq!(matches.len(), 3);
    assert_eq!(matches[0].show(), ["__1 => a"]);
    assert_eq!(matches[1].show(), ["__1 => b"]);
    assert_eq!(matches[2].show(), ["__1 => c"]);

    let items = parse!(a [] b);
    let pat = parse!(a [] b);
    let matches = find_all(&items, &pat);
    assert_eq!(matches.len(), 1);
}

#[test]
fn test_non_greedy_matches() {
    // "a" after "___1" cannot match the longest "a".
    let items = parse!(a b a c a d);
    let pat = parse!(a ___1 a ___2 c ___3 d);
    let matches = find_all(&items, &pat);
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].show(), ["___1 => b", "___2 => ", "___3 => a"]);

    // [ ___2 i ___3 ] match the middle, not the first nor the last.
    let items = parse!(a b c d [ e f g ] [ h i j ] [ k l m ]);
    let pat = parse!(a ___1g [ ___2 i ___3 ]);
    let matches = find_all(&items, &pat);
    assert_eq!(matches.len(), 1);
    assert_eq!(
        matches[0].show(),
        ["___1g => b c d [ e f g ]", "___2 => h", "___3 => j"]
    );
}

#[test]
fn test_find_all_multiple_matches() {
    let items = parse!(a b a c);
    let pat = parse!(a __1);
    let matches = find_all(&items, &pat);
    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0].show(), ["__1 => b"]);
    assert_eq!(matches[1].show(), ["__1 => c"]);

    // "e" is ignored, since matches are picked from the start.
    let items = parse!(a b c d e);
    let pat = parse!(__1 __2);
    let matches = find_all(&items, &pat);
    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0].show(), ["__1 => a", "__2 => b"]);
    assert_eq!(matches[1].show(), ["__1 => c", "__2 => d"]);

    let items = parse!(a b a c [] a x [] a);
    let pat = parse!(a ___1).with_placeholder_matching_items([(
        "___1",
        (|t: &Item| !matches!(t, Item::Tree(..))) as fn(&Item) -> bool,
    )]);
    let matches = find_all(&items, &pat);
    assert_eq!(matches.len(), 3);
    assert_eq!(matches[0].show(), ["___1 => b a c"]);
    assert_eq!(matches[1].show(), ["___1 => x"]);
    assert_eq!(matches[2].show(), ["___1 => "]);
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

    let matches = find_all(&parse!(a b c d [ e f ]), &parse!(a ___1 d [ ___2 ]));
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].show(), ["___1 => b c", "___2 => e f"]);

    let items = parse!(a [ a [ a [ x ] ] ] b [ a [ y ] b [ a [ p ] a [ q ] ] ]);
    let pat = parse!(a[___1g]);
    let matches = find_all(&items, &pat);
    assert_eq!(matches.len(), 4);
    assert_eq!(matches[0].show(), ["___1g => a [ a [ x ] ]"]);
    assert_eq!(matches[1].show(), ["___1g => y"]);
    assert_eq!(matches[2].show(), ["___1g => p"]);
    assert_eq!(matches[3].show(), ["___1g => q"]);
}

#[test]
fn test_find_all_greedy_matches() {
    let items = parse!(a b c);
    let pat = parse!(___1 c);
    let matches = find_all(&items, &pat);
    // ___1 includes both "a" and "b".
    assert_eq!(matches[0].show(), ["___1 => a b"]);
}

#[test]
fn test_find_one() {
    let items = parse!(a b c);

    // Match align with the start.
    let pat = parse!(a __1);
    let m = matches_start(&items, &pat);
    assert_eq!(m.unwrap().show(), ["__1 => b"]);

    let pat = parse!(b __1);
    let m = matches_start(&items, &pat);
    assert!(m.is_none());

    // Match align with the start and end.
    let pat = parse!(a __1);
    let m = matches_full(&items, &pat);
    assert!(m.is_none());

    let pat = parse!(b ___1);
    let m = matches_full(&items, &pat);
    assert!(m.is_none());

    let pat = parse!(a ___1);
    let m = matches_full(&items, &pat);
    assert_eq!(m.unwrap().show(), ["___1 => b c"]);
}

#[test]
fn test_find_all_with_custom_placeholders() {
    let items = parse!(a b c xx d e f xx x y z);
    let pat = parse!(___1 xx ___2).with_placeholder_matching_items([
        (
            "___1",
            (|item: &Item| matches!(item, Item::Item(x) if x != "xx")) as fn(&Item) -> _,
        ),
        (
            "___2",
            (|item: &Item| matches!(item, Item::Item(x) if x != "f")) as fn(&Item) -> _,
        ),
    ]);
    let matches = find_all(&items, &pat);
    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0].show(), ["___1 => a b c", "___2 => d e"]);
    assert_eq!(matches[1].show(), ["___1 => f", "___2 => x y z"]);
}

#[test]
fn test_replace_all() {
    let items = parse!(async fn foo [B is Future [T] Send] (a b) Future [ R ] { a dot await and b dot await });
    let items = replace_all(&items, &parse!(dot await), parse!());
    assert_eq!(
        unparse(&items),
        "async fn foo [ B is Future [ T ] Send ] ( a b ) Future [ R ] { a and b }"
    );
    let items = replace_all(&items, &parse!([ B ___g1 Send ]), parse!());
    assert_eq!(
        unparse(&items),
        "async fn foo ( a b ) Future [ R ] { a and b }"
    );
    let items = replace_all(&items, &parse!(Future[__1]), parse!(__1));
    assert_eq!(unparse(&items), "async fn foo ( a b ) R { a and b }");
    let items = replace_all(&items, &parse!(a ___1 b), parse!(b ___1 a));
    assert_eq!(unparse(&items), "async fn foo ( b a ) R { b and a }");
}

#[test]
fn test_replace_all_adjacent() {
    let items = parse!(a b c);
    let items = replace_all(&items, &parse!(__1), parse!(__1 dot));
    assert_eq!(unparse(&items), "a dot b dot c dot");
}

#[test]
fn test_replace_all_nested() {
    let items = parse!(x a b x [ x a [ x a c ] x ]);
    // Swap "a" and its next item.
    let items = replace_all(&items, &parse!(a __1g), parse!(__1g a));
    // The "a [ x a c ]" was swapped so the inner "x a c" is not changed.
    assert_eq!(unparse(&items), "x b a x [ x [ x a c ] a x ]");
}

#[test]
fn test_replace_func() {
    let items = parse!(x [a b] x [c d e] x);
    let pat = parse!([___1]);
    let replaced = replace_all(&items, &pat, |m: &Match<String>| -> Vec<Item> {
        let mut v: Vec<Item> = m.captures["___1"].clone();
        v.reverse();
        v
    });
    assert_eq!(unparse(&replaced), "x b a x e d c x");
}
