/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::borrow::Cow;

use configmodel::Config;
use configmodel::Text;
use configmodel::ValueSource;
pub use phf;
use phf::OrderedMap;
pub use staticconfig_macros as _detail;

/// Define a `StaticConfig` that implements the `Config` trait.
/// A static config has no runtime cost on parsing or constructing a hash map.
///
/// Supports two formats: hgrc and json-like.
///
/// hgrc is an INI-like format with extra features like `%unset`. For example:
///
/// ```ignore
/// const CONFIG1: StaticConfig = static_config!(r"""
/// [section]
/// name = value
/// """);
/// ```
///
/// json-like describes configs in `{ section: { name: value } }` format
/// where section, name, value are strings. For example:
///
/// ```ignore
/// const CONFIG2: StaticConfig = static_config!{
///     "section": {
///         "name": "value"
///     }
/// };
/// ```
///
/// To assign a name to the config, prefix it with `name =>`. For example:
///
/// ```ignore
/// const CONFIG1: StaticConfig = static_config! {
///     "my_config" => r"""
/// [section]
/// name = value
/// """
/// };
///
/// const CONFIG2: StaticConfig = static_config! {
///     "my_config" => { "section": { "name": "value" } }
/// };
/// ```
#[macro_export]
macro_rules! static_config {
    // json-like { section: { name: value } }
    { $( $section:literal :
         { $(  $name:literal : $value: literal ),* $(,)? }
       ),* $(,)?
    } => {
        staticconfig::_detail::static_items![
            $( $( ( $section, $name, $value ), )* )*
        ]
    };

    // hgrc-like
    ( $text:literal ) => {
        staticconfig::_detail::static_rc!($text)
    };

    // with a name
    ( $name:literal => { $( $body:tt )* } ) => {
        static_config! { $($body)* }.named($name)
    };
    ( $name:literal => $text:literal ) => {
        static_config!($text).named($name)
    };
}

/// Statically compiled config that does not require runtime parsing.
/// Can be used for testing too.
///
/// Use `static_config!` to construct this type.
pub struct StaticConfig {
    name: &'static str,
    sections: OrderedMap<&'static str, OrderedMap<&'static str, Option<&'static str>>>,
}

impl StaticConfig {
    /// Change the "name" of this config.
    pub const fn named(self, name: &'static str) -> Self {
        Self {
            name,
            sections: self.sections,
        }
    }

    /// Construct `StaticConfig`. This is intended to be used only by the
    /// `static_rc!` macro.
    pub const fn from_macro_rules(
        sections: OrderedMap<&'static str, OrderedMap<&'static str, Option<&'static str>>>,
    ) -> Self {
        Self {
            name: "StaticConfig",
            sections,
        }
    }
}

impl Config for StaticConfig {
    fn keys(&self, section: &str) -> Vec<Text> {
        match self.sections.get(section) {
            Some(map) => map.keys().map(|n| Text::from_static(n)).collect(),
            None => Vec::new(),
        }
    }

    fn get_considering_unset(&self, section: &str, name: &str) -> Option<Option<Text>> {
        match self.sections.get(section)?.get(name) {
            None => None,
            Some(None) => Some(None),
            Some(Some(value)) => Some(Some(Text::from_static(value))),
        }
    }

    fn sections(&self) -> Cow<'_, [Text]> {
        let sections: Vec<Text> = self.sections.keys().map(|n| Text::from_static(n)).collect();
        sections.into()
    }

    fn get_sources(&self, section: &str, name: &str) -> Cow<'_, [ValueSource]> {
        match self.get_considering_unset(section, name) {
            Some(value) => Cow::Owned(vec![ValueSource {
                value,
                source: Text::from_static(self.name),
                location: None,
            }]),
            None => Cow::Borrowed(&[]),
        }
    }

    fn layer_name(&self) -> Text {
        Text::from_static(self.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Emulates external crate usage where `staticconfig::StaticConfig` is available.
    // This is needed, since `static_rc!` refers to `staticconfig::StaticConfig`.
    pub mod staticconfig {
        pub use crate::_detail;
        pub use crate::StaticConfig;
        pub use crate::phf;
    }

    #[test]
    fn test_rc_format() {
        const CONFIG: StaticConfig = static_config!(
            r#"
[a1]
v1 = x
v3 = y
v2 = z
v2 = zz

[c3]
foo = bar

[b2]
multi-line = line1
  line2
  line3
            "#
        )
        .named("test.rc");

        check_config(&CONFIG);
    }

    #[test]
    fn test_unset() {
        let config = static_config!(
            r#"
[a]
v1 = x
%unset v1
%unset v2
v2 = x
%unset v3
            "#
        );

        assert_eq!(config.get_considering_unset("a", "v1"), Some(None));
        assert_eq!(
            config.get_considering_unset("a", "v2").unwrap().unwrap(),
            "x"
        );
        assert_eq!(config.get_considering_unset("b", "v2"), None);
        assert_eq!(config.get_considering_unset("a", "v3"), Some(None));
        assert_eq!(config.get_considering_unset("a", "v4"), None);

        let sources = &config.get_sources("a", "v1")[0];
        assert_eq!(sources.value, None);
        assert_eq!(sources.source, config.name);
        let sources = &config.get_sources("a", "v2")[0];
        assert_eq!(sources.value.as_deref(), Some("x"));
        assert_eq!(sources.source, config.name);
        let sources = &config.get_sources("a", "v3")[0];
        assert_eq!(sources.value, None);
        assert_eq!(sources.source, config.name);
        assert!(config.get_sources("a", "v4").is_empty());
    }

    #[test]
    fn test_json_like() {
        const CONFIG: StaticConfig = static_config! {
            "a1": {
                "v1": "x",
                "v3": "y",
                "v2": "z",
                "v2": "zz",
            },
            "c3": {
                "foo": "bar",
            },
            "b2": {
                "multi-line": "line1\nline2\nline3",
            },
        };

        check_config(&CONFIG);
    }

    #[test]
    fn test_with_name() {
        const CONFIG: StaticConfig = static_config!("a.rc" => "[a]\nb=1");
        assert_eq!(CONFIG.name, "a.rc");
        assert_eq!(CONFIG.get("a", "b").unwrap(), "1");

        let config: StaticConfig = static_config!("b.rc" => {"a": {"b": "1"}});
        assert_eq!(config.name, "b.rc");
        assert_eq!(config.get("a", "b").unwrap(), "1");
    }

    /// Check config equivalent to the one in `test_static_rc`.
    fn check_config(config: &StaticConfig) {
        // sections() works and preserves order.
        assert_eq!(config.sections().into_owned(), ["a1", "c3", "b2"]);

        // keys() works and preserves order.
        assert_eq!(config.keys("a1"), ["v1", "v3", "v2"]);

        // get() works.
        assert_eq!(
            config.get("b2", "multi-line").unwrap(),
            "line1\nline2\nline3"
        );
        assert_eq!(config.get("a1", "v3").unwrap(), "y");
        assert_eq!(config.get("a1", "v2").unwrap(), "zz");

        assert!(config.get("a1", "v4").is_none());
        assert!(config.get("c2", "foo").is_none());

        // layer_name() defined by `named`.
        assert_eq!(config.layer_name(), config.name);

        // get_source() provides the name.
        assert!(config.get_sources("c2", "foo").is_empty());
        let sources = config.get_sources("a1", "v2");
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].source, config.name);
        assert_eq!(sources[0].value.as_deref(), Some("zz"));
    }
}
