// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#[macro_export]
macro_rules! define_flags {
    ( $( $vis:vis struct $name:ident { $( $token:tt )* } )*  ) => {
        $( $crate::_define_flags_impl!([ $( $token )* ] [] [] ($vis $name) ); )*
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! _define_flags_impl {
    // A recursive macro that has states.
    //
    //  ( [...]          [ (short, field, doc, type, default)... ] [args] (vis  name) )
    //    ^^^^^          ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    //    input          parsed state

    // Nothing left to parse
    ( [] [ $( ($short:literal, $field:ident, $doc:expr, $type:ty, $default:expr) )* ] [ $($args:ident)? ] ($vis:vis $name:ident) ) => {
        $vis struct $name {
            $( #[doc=$doc] pub $field : $type , )*
            $( pub $args: Vec<String>, )?
        }

        impl $crate::parser::StructFlags for $name {
            fn flags() -> Vec<$crate::parser::Flag> {
                vec![
                    $( ($short, stringify!($field).replace("_", "-"), $doc.trim().to_string(), $crate::parser::Value::from($default)), )*
                ].into_iter().map(Into::into).collect()
            }
        }

        impl From<$crate::parser::ParseOutput> for $name {
            fn from(out: $crate::parser::ParseOutput) -> Self {
                Self {
                    $( $field : out.pick::<$type>(&stringify!($field).replace("_", "-")), )*
                    $( $args: out.args, )?
                }
            }
        }
    };

    // Match a field like:
    //
    //    /// description
    //    name: type,
    ( [ #[doc=$doc:expr] $field:ident : $type:ty, $($rest:tt)* ] [ $( $parsed:tt )* ] [ $($args:ident)? ] $tail:tt ) => {
        $crate::_define_flags_impl!( [ $( $rest )* ]
                                     [ $( $parsed )* (' ', $field, $doc, $type, (<$type>::default())) ]
                                     [ $( $args )? ]
                                     $tail);
    };

    // Match a field like:
    //
    //    /// description
    //    name: type = default,
    ( [ #[doc=$doc:expr] $field:ident : $type:ty = $default:tt, $($rest:tt)* ] [ $( $parsed:tt )* ] [ $($args:ident)? ] $tail:tt ) => {
        $crate::_define_flags_impl!( [ $( $rest )* ]
                                     [ $( $parsed )* (' ', $field, $doc, $type, $default) ]
                                     [ $( $args )? ]
                                     $tail);
    };

    // Match a field like:
    //
    //    /// description
    //    #[short('s')]
    //    name: type,
    ( [ #[doc=$doc:expr] #[short($short:literal)] $field:ident : $type:ty, $($rest:tt)* ] [ $( $parsed:tt )* ] [ $($args:ident)? ] $tail:tt ) => {
        $crate::_define_flags_impl!( [ $( $rest )* ]
                                     [ $( $parsed )* ($short, $field, $doc, $type, (<$type>::default())) ]
                                     [ $( $args )? ]
                                     $tail);
    };

    // Match a field like:
    //
    //    /// description
    //    #[short('s')]
    //    name: type = default,
    ( [ #[doc=$doc:expr] #[short($short:literal)] $field:ident : $type:ty = $default:tt, $($rest:tt)* ] [ $( $parsed:tt )* ] [ $($args:ident)? ] $tail:tt ) => {
        $crate::_define_flags_impl!( [ $( $rest )* ]
                                     [ $( $parsed )* ($short, $field, $doc, $type, $default) ]
                                     [ $( $args )? ]
                                     $tail);
    };

    // Match a field like:
    //
    //    #[args]
    //    patterns: Vec<String>,
    ( [ #[args] $args_name:ident : Vec<String>, $($rest:tt)* ] [ $( $parsed:tt )* ] [ ] $tail:tt ) => {
        $crate::_define_flags_impl!( [ $( $rest )* ]
                                     [ $( $parsed )* ]
                                     [ $args_name ]
                                     $tail);
    };
}

#[cfg(test)]
mod tests {
    define_flags! {
        struct TestOptions {
            /// bool value
            boo: bool = true,

            /// foo
            foo: bool,

            /// int value
            count: i64 = 12,

            /// name
            long_name: String = "alice",

            /// revisions
            #[short('r')]
            rev: Vec<String>,
        }

        struct AnotherTestOptions {
            /// follow renames
            follow: bool,

            #[args]
            pats: Vec<String>,
        }
    }

    use crate::parser::{Flag, ParseOptions, StructFlags, Value};

    #[test]
    fn test_struct_flags() {
        let flags = TestOptions::flags();
        let expected: Vec<Flag> = vec![
            (None, "boo", "bool value", Value::from(true)),
            (None, "foo", "foo", Value::from(false)),
            (None, "count", "int value", Value::from(12)),
            (None, "long-name", "name", Value::from("alice")),
            (Some('r'), "rev", "revisions", Value::from(Vec::new())),
        ]
        .into_iter()
        .map(Into::into)
        .collect();
        assert_eq!(flags, expected);

        let flags = AnotherTestOptions::flags();
        assert_eq!(flags.len(), 1);
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
        assert_eq!(parsed.long_name, "alice");
        assert!(parsed.rev.is_empty());

        let parsed = ParseOptions::new()
            .flags(TestOptions::flags())
            .parse_args(&vec!["--no-boo", "--long-name=bob", "--rev=b", "-r", "a"])
            .unwrap();
        let parsed = TestOptions::from(parsed);
        assert_eq!(parsed.boo, false);
        assert_eq!(parsed.foo, false);
        assert_eq!(parsed.count, 12);
        assert_eq!(parsed.long_name, "bob");
        assert_eq!(parsed.rev, vec!["b", "a"]);

        let parsed = ParseOptions::new()
            .flags(AnotherTestOptions::flags())
            .parse_args(&vec!["--no-follow", "b", "--follow", "c"])
            .unwrap();
        let parsed = AnotherTestOptions::from(parsed);
        assert_eq!(parsed.follow, true);
        assert_eq!(parsed.pats, vec!["b", "c"]);
    }
}
