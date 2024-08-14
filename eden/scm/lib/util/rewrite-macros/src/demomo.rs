/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::prelude::*;

// arg         | inner_arg | pass_arg      | inner_body
// --------------------------------------------------------------
// x: AsRef<T> | x: &T     | x.as_ref()    | x.as_ref() => x
// x: Into<T>  | x: T      | x.into()      | x.into() => x
// x: ToString | x: String | x.to_string() | x.to_string() => x

struct ImplTemplate {
    from: &'static str,
    to: &'static str,
    body: &'static str,
}

const IMPL_TEMPLATES: &[ImplTemplate] = &[
    ImplTemplate {
        from: "AsRef<___Tg>",
        to: "&___Tg",
        body: ".as_ref()",
    },
    ImplTemplate {
        from: "Into<___Tg>",
        to: "___Tg",
        body: ".into()",
    },
    ImplTemplate {
        from: "ToString",
        to: " String",
        body: ".to_string()",
    },
];

pub(crate) fn demomo(attr: TokenStream, tokens: TokenStream) -> TokenStream {
    let debug = !attr.find_all(parse("debug")).is_empty();
    let self_type: Vec<Item> = tokens
        .find_all("impl __TYPE { ____g }")
        .first()
        .map_or_else(
            || parse("Self").to_items(),
            |m| m.captures["__TYPE"].clone(),
        );
    let pat = "fn __NAME (___ARGSg) ___RET { ___BODYg } "
        .to_items()
        .disallow_group_match("___RET");
    tokens.replace_with(pat, |m: &Match<TokenInfo>| {
        let name = &m.captures["__NAME"];
        let args = m.captures["___ARGSg"]
            .replace("___PREFIX self,", "self: ___PREFIX Self,")
            .group_by_angle_bracket();
        let ret = &m.captures["___RET"];
        let inner_ret = ret.replace("Self", self_type.clone());
        let body = &m.captures["___BODYg"];
        let inner_name = {
            let mut body_args = body.clone();
            body_args.extend(args.clone());
            pick_unique_name(body_args, "inner")
        };

        if debug {
            eprintln!(
                "name: [{}], args: [{}], ret: [{}]",
                unparse(name),
                unparse(&args),
                unparse(ret)
            );
        }

        let mut inner_args = args
            .replace("Self", self_type.clone())
            .replace("self", "self_")
            .group_by_angle_bracket();
        let mut inner_body = body
            .replace("Self", self_type.clone())
            .replace("self", "self_");
        let mut pass_args = scan_names(&args, &"__NAME:".to_items()).replace("__N", "__N,");

        for t in IMPL_TEMPLATES {
            let from = parse(&format!("__NAME: impl {}", t.from))
                .to_items()
                .group_by_angle_bracket();
            let to = format!("__NAME: {}", t.to);
            let names = scan_names(&inner_args, &from);
            if names.is_empty() {
                continue;
            }
            // inner_args: x: AsRef<T> => x: &T
            inner_args = inner_args.replace(&*from, &*to);
            if debug {
                eprintln!("- {}: {:?}", t.from.replace("___Tg", "T"), unparse(&names),);
            }
            for name in names {
                // inner_body: x.as_ref() => x
                let mut long = vec![name.clone()];
                long.extend(parse(t.body).to_items());
                let short = vec![name.clone()];
                inner_body = inner_body.replace(long.clone(), short.clone());
                // pass_args: x => x.as_ref()
                pass_args = pass_args.replace(short, long)
            }
        }

        let tokens = {
            let name = name.to_tokens();
            let args = &args.to_tokens();
            let ret = ret.to_tokens();
            let inner_ret = &inner_ret.to_tokens();
            let inner_args = &inner_args.to_tokens();
            let inner_body = &inner_body.to_tokens();
            let pass_args = &pass_args.to_tokens();

            quote! {
                fn #name ( #args ) #ret {
                    fn #inner_name ( #inner_args ) #inner_ret {
                        #inner_body
                    }
                    #inner_name ( #pass_args )
                }
            }
        };

        if debug {
            eprintln!("output: [[[\n{}\n]]]", unparse(&tokens));
        }

        tokens.to_items()
    })
}

fn scan_names(items: &Vec<Item>, pattern: &Vec<Item>) -> Vec<Item> {
    items
        .find_all(pattern)
        .into_iter()
        .map(|m| {
            let mut items = m.captures["__NAME"].clone();
            assert_eq!(items.len(), 1);
            items.pop().unwrap()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_demomo_as_ref() {
        // Add "debug" to "attr" to debug tests.
        let attr = parse("");
        let code = parse(
            r#"
            fn f(p: impl AsRef<Path>) -> String {
                read(p.as_ref())
            }
"#,
        );
        assert_eq!(
            unparse(&demomo(attr, code)),
            r#"
            fn f (p : impl AsRef < Path >) -> String {
                fn inner (p : & Path) -> String { read (p) } inner (p . as_ref () ,)
            }"#
        );
    }

    #[test]
    fn test_demomo_to_string() {
        let attr = parse("");
        let code = parse(
            r#"
            fn f(x: impl ToString) -> usize {
                let x = x.to_string();
                x.len()
            }
"#,
        );
        assert_eq!(
            unparse(&demomo(attr, code)),
            r#"
            fn f (x : impl ToString) -> usize {
                fn inner (x : String) -> usize {
                    let x = x;
                    x . len ()
                }
                inner (x . to_string () ,)
            }"#
        );
    }

    #[test]
    fn test_demomo_to_into() {
        let attr = parse("");
        let code = parse(
            r#"
            fn f(x: impl Into<Vec<u8>>) -> Vec<u8> {
                x.into().to_vec()
            }
"#,
        );
        assert_eq!(
            unparse(&demomo(attr, code)),
            r#"
            fn f (x : impl Into < Vec < u8 >>) -> Vec < u8 > {
                fn inner (x : Vec < u8 >) -> Vec < u8 > {
                    x . to_vec ()
                }
                inner (x . into () ,)
            }"#
        );
    }

    #[test]
    fn test_demomo_multiple_args() {
        let attr = parse("");
        let code = parse(
            r#"
            fn f(x: impl AsRef<Path>, y: impl ToString, z: impl Into<Vec<u8>>) -> Result<usize> {
                let path = x.as_ref().join(y.to_string());
                let content = z.into();
                write(path, content)
            }
"#,
        );
        assert_eq!(
            unparse(&demomo(attr, code)),
            r#"
            fn f (x : impl AsRef < Path >, y : impl ToString , z : impl Into < Vec < u8 >>) -> Result < usize > {
                fn inner (x : & Path , y : String , z : Vec < u8 >) -> Result < usize > {
                    let path = x . join (y);
                    let content = z;
                    write (path , content)
                }
                inner (x . as_ref () , y . to_string () , z . into () ,)
            }"#
        );
    }

    #[test]
    fn test_inner_name() {
        let attr = parse("");
        let code = parse(
            r#"
            fn f(x: impl ToString, inner_: T) {
                fn inner() { }
                fn inner__() { }
                dbg!(x);
            }
            "#,
        );
        assert_eq!(
            unparse(&demomo(attr, code)),
            r#"
            fn f (x : impl ToString , inner_ : T) {
                fn inner___ (x : String , inner_ : T) {
                    fn inner () { } fn inner__ () { } dbg ! (x);
                }
                inner___ (x . to_string () , inner_ ,)
            }"#
        );
    }

    #[test]
    fn test_demomo_self() {
        let attr = parse("");
        let code = parse(
            r#"
            impl Foo {
                fn f(&mut self, x: impl ToString) -> &mut Self {
                    self.x = x.to_string();
                    self
                }
            }
"#,
        );
        assert_eq!(
            unparse(&demomo(attr, code)),
            r#"
            impl Foo {
                fn f (self : & mut Self , x : impl ToString) -> & mut Self {
                    fn inner (self_ : & mut Foo , x : String) -> & mut Foo {
                        self_ . x = x;
                        self_
                    }
                    inner (self , x . to_string () ,)
                }
            }"#
        );
    }

    #[test]
    fn test_group_in_return_type() {
        let attr = parse("");
        let code = parse(
            r#"
            fn f(x: impl ToString) -> Result<()> {
                dbg!(x.to_string());
                Ok(())
            }
"#,
        );
        assert_eq!(
            unparse(&demomo(attr, code)),
            r#"
            fn f (x : impl ToString) -> Result < () > {
                fn inner (x : String) -> Result < () > {
                    dbg ! (x);
                    Ok (())
                }
                inner (x . to_string () ,)
            }"#
        );
    }
}
