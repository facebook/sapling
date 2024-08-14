/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::prelude::*;

pub(crate) fn syncify(attr: TokenStream, mut tokens: TokenStream) -> TokenStream {
    let debug = !attr.find_all(parse("debug")).is_empty();
    tokens = tokens
        .replace(parse(".await"), parse(""))
        .replace(parse(".boxed()"), parse(""))
        .replace(parse("async move"), parse(""))
        .replace(parse("async"), parse(""))
        .replace(parse("#[tokio::test]"), parse("#[test]"))
        .replace(parse("__::block_on(___g1)"), parse("___g1"));

    // Apply customized replaces.
    let matches = attr.find_all(parse("[___g1] => [___g2]"));
    if debug {
        eprintln!("{} customized replaces", matches.len());
    }
    for m in matches {
        let pat = m.captures.get("___g1").unwrap();
        let replace = m.captures.get("___g2").unwrap();
        tokens = tokens.replace(pat, replace);
    }

    // `cargo expand` can also be used to produce output.
    if debug {
        eprintln!("output: [[[\n{}\n]]]", unparse(&tokens));
    }

    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_syncify_basic() {
        let attr = parse("");
        let code = parse(
            r#"
            async fn foo(x: usize) -> usize {
                async_runtime::block_on(async { bar(x).await + 1 })
            }
"#,
        );
        assert_eq!(
            unparse(&syncify(attr, code)),
            r#"
            fn foo (x : usize) -> usize {
                { bar (x) + 1 }
            }"#
        );
    }

    #[test]
    fn test_syncify_tests() {
        let attr = parse("");
        let code = parse(
            r#"
            #[tokio::test]
            async fn test_foo() {
                assert!(g().await);
            }
"#,
        );
        assert_eq!(
            unparse(&syncify(attr, code)),
            r#"
            # [test] fn test_foo () {
                assert ! (g ());
            }"#
        );
    }
}
