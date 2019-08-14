// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#[macro_export]
macro_rules! define_flags {
    ($vis:vis struct $name:ident { $( #[doc=$doc:expr] $field:ident : $type:ty = $default:tt , )* } ) => {
        $vis struct $name {
            $( #[doc=$doc] pub $field : $type , )*
        }

        impl $crate::parser::StructFlags for $name {
            fn flags() -> Vec<$crate::parser::Flag> {
                vec![
                    $( (None, stringify!($field), $doc.trim(), $crate::parser::Value::from($default)), )*
                ].into_iter().map(Into::into).collect()
            }
        }

        impl From<$crate::parser::ParseOutput> for $name {
            fn from(out: $crate::parser::ParseOutput) -> Self {
                Self {
                    $( $field : out.get(stringify!($field)).cloned().unwrap().into() , )*
                }
            }
        }
    };
}

#[cfg(test)]
mod tests {
    define_flags! {
        struct TestOptions {
            /// bool value
            boo: bool = true,

            /// int value
            count: i64 = 12,

            /// name
            name: String = "alice",

            /// revisions
            rev: Vec<String> = (),
        }
    }

    use crate::parser::{Flag, ParseOptions, StructFlags, Value};

    #[test]
    fn test_struct_flags() {
        let flags = TestOptions::flags();
        let expected: Vec<Flag> = vec![
            (None, "boo", "bool value", Value::from(true)),
            (None, "count", "int value", Value::from(12)),
            (None, "name", "name", Value::from("alice")),
            (None, "rev", "revisions", Value::from(())),
        ]
        .into_iter()
        .map(Into::into)
        .collect();
        assert_eq!(flags, expected);
    }

    #[test]
    fn test_struct_parse() {
        let parsed = ParseOptions::new()
            .flags(TestOptions::flags())
            .parse_args(&vec!["--count", "3"])
            .unwrap();
        let parsed = TestOptions::from(parsed);
        assert_eq!(parsed.boo, true);
        assert_eq!(parsed.count, 3);
        assert_eq!(parsed.name, "alice");
        assert!(parsed.rev.is_empty());

        let parsed = ParseOptions::new()
            .flags(TestOptions::flags())
            .parse_args(&vec!["--no-boo", "--name=bob", "--rev=b", "--rev", "a"])
            .unwrap();
        let parsed = TestOptions::from(parsed);
        assert_eq!(parsed.boo, false);
        assert_eq!(parsed.count, 12);
        assert_eq!(parsed.name, "bob");
        assert_eq!(parsed.rev, vec!["b", "a"]);
    }
}
