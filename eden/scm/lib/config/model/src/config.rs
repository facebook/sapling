/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::ops::Range;
use std::path::PathBuf;
use std::str;
use std::sync::Arc;

use minibytes::Text;

use crate::convert::FromConfigValue;
use crate::Result;

/// Readable config. This can be used as a trait object.
#[auto_impl::auto_impl(&, Box, Arc)]
pub trait Config: Send + Sync {
    /// Get config names in the given section. Sorted by insertion order.
    fn keys(&self, section: &str) -> Vec<Text>;

    /// Keys with the given prefix.
    fn keys_prefixed(&self, section: &str, prefix: &str) -> Vec<Text> {
        self.keys(section)
            .into_iter()
            .filter(|k| k.starts_with(prefix))
            .collect()
    }

    /// Get config value for a given config.
    /// Return `None` if the config item does not exist or is unset.
    fn get(&self, section: &str, name: &str) -> Option<Text> {
        self.get_considering_unset(section, name)?
    }

    /// Similar to `get`, but can represent "%unset" result.
    /// - `None`: not set or unset.
    /// - `Some(None)`: unset.
    /// - `Some(Some(value))`: set.
    fn get_considering_unset(&self, section: &str, name: &str) -> Option<Option<Text>>;

    /// Get a nonempty config value for a given config.
    /// Return `None` if the config item does not exist, is unset or is empty str.
    fn get_nonempty(&self, section: &str, name: &str) -> Option<Text> {
        self.get(section, name).filter(|v| !v.is_empty())
    }

    /// Get config sections.
    fn sections(&self) -> Cow<[Text]>;

    /// Get the sources of a config.
    fn get_sources(&self, section: &str, name: &str) -> Cow<[ValueSource]>;

    /// Get on-disk files loaded for this `Config`.
    fn files(&self) -> Cow<[PathBuf]> {
        Cow::Borrowed(&[])
    }

    /// Break the config into (immutable) layers.
    ///
    /// If returns an empty list, then the config object is considered atomic.
    ///
    /// If returns a list, then those are considered "sub"-configs that this
    /// config will consider. The order matters. Config with a larger index
    /// overrides configs with smaller indexes. Note the combination of all
    /// sub-configs might not be equivalent to the "self" config, since
    /// there might be some overrides.
    fn layers(&self) -> Vec<Arc<dyn Config>> {
        Vec::new()
    }

    /// The name of the current layer.
    fn layer_name(&self) -> Text;
}

/// Extra APIs (incompatible with trait objects) around reading config.
pub trait ConfigExt: Config {
    /// Get a config item. Convert to type `T`.
    fn get_opt<T: FromConfigValue>(&self, section: &str, name: &str) -> Result<Option<T>> {
        self.get(section, name)
            .map(|bytes| T::try_from_str(&bytes))
            .transpose()
    }

    /// Get a nonempty config item. Convert to type `T`.
    fn get_nonempty_opt<T: FromConfigValue>(&self, section: &str, name: &str) -> Result<Option<T>> {
        self.get_nonempty(section, name)
            .map(|bytes| T::try_from_str(&bytes))
            .transpose()
    }

    /// Get a config item. Convert to type `T`.
    ///
    /// If the config item is not set, calculate it using `default_func`.
    fn get_or<T: FromConfigValue>(
        &self,
        section: &str,
        name: &str,
        default_func: impl Fn() -> T,
    ) -> Result<T> {
        Ok(self.get_opt(section, name)?.unwrap_or_else(default_func))
    }

    /// Get a config item. Convert to type `T`.
    ///
    /// If the config item is not set, return `T::default()`.
    fn get_or_default<T: Default + FromConfigValue>(&self, section: &str, name: &str) -> Result<T> {
        self.get_or(section, name, Default::default)
    }
}

impl<T: Config> ConfigExt for T {}

impl Config for BTreeMap<&str, &str> {
    fn keys(&self, section: &str) -> Vec<Text> {
        let prefix = format!("{}.", section);
        BTreeMap::keys(self)
            .filter_map(|k| k.strip_prefix(&prefix).map(|k| k.to_string().into()))
            .collect()
    }

    fn sections(&self) -> Cow<[Text]> {
        let mut sections = Vec::new();
        let mut last_section = None;
        for section in BTreeMap::keys(self).filter_map(|k| k.split('.').next()) {
            if Some(section) != last_section {
                last_section = Some(section);
                sections.push(Text::from(section.to_string()));
            }
        }
        Cow::Owned(sections)
    }

    fn get_considering_unset(&self, section: &str, name: &str) -> Option<Option<Text>> {
        let key: &str = &format!("{}.{}", section, name);
        BTreeMap::get(self, &key).map(|v| Some(v.to_string().into()))
    }

    fn get_sources(&self, section: &str, name: &str) -> Cow<[ValueSource]> {
        match Config::get(self, section, name) {
            None => Cow::Borrowed(&[]),
            Some(value) => Cow::Owned(vec![ValueSource {
                value: Some(value),
                source: Text::from_static("BTreeMap"),
                location: None,
            }]),
        }
    }

    fn layer_name(&self) -> Text {
        Text::from_static("BTreeMap")
    }
}

impl Config for BTreeMap<String, String> {
    fn keys(&self, section: &str) -> Vec<Text> {
        let prefix = format!("{}.", section);
        BTreeMap::keys(self)
            .filter_map(|k| k.strip_prefix(&prefix).map(|k| k.to_string().into()))
            .collect()
    }

    fn sections(&self) -> Cow<[Text]> {
        let mut sections = Vec::new();
        let mut last_section = None;
        for section in BTreeMap::keys(self).filter_map(|k| k.split('.').next()) {
            if Some(section) != last_section {
                last_section = Some(section);
                sections.push(Text::from(section.to_string()));
            }
        }
        Cow::Owned(sections)
    }

    fn get_considering_unset(&self, section: &str, name: &str) -> Option<Option<Text>> {
        BTreeMap::get(self, &format!("{}.{}", section, name)).map(|v| Some(v.clone().into()))
    }

    fn get_sources(&self, section: &str, name: &str) -> Cow<[ValueSource]> {
        match Config::get(self, section, name) {
            None => Cow::Borrowed(&[]),
            Some(value) => Cow::Owned(vec![ValueSource {
                value: Some(value),
                source: Text::from_static("BTreeMap"),
                location: None,
            }]),
        }
    }

    fn layer_name(&self) -> Text {
        Text::from_static("BTreeMap")
    }
}

/// A config value with associated metadata like where it comes from.
#[derive(Clone, Debug)]
pub struct ValueSource {
    pub value: Option<Text>,
    pub source: Text, // global, user, repo, "--config", or an extension name, etc.
    pub location: Option<ValueLocation>,
}

/// The on-disk file name and byte offsets that provide the config value.
/// Useful if applications want to edit config values in-place.
#[derive(Clone, Debug)]
pub struct ValueLocation {
    pub path: Arc<PathBuf>,
    pub content: Text,
    pub location: Range<usize>,
}

impl ValueSource {
    /// Return the actual value stored in this config value, or `None` if uset.
    pub fn value(&self) -> &Option<Text> {
        &self.value
    }

    /// Return the "source" information for the config value. It's usually who sets the config,
    /// like "--config", "user_hgrc", "system_hgrc", etc.
    pub fn source(&self) -> &Text {
        &self.source
    }

    /// Return the file path and byte range for the exact config value,
    /// or `None` if there is no such information.
    ///
    /// If the value is `None`, the byte range is for the "%unset" statement.
    pub fn location(&self) -> Option<(PathBuf, Range<usize>)> {
        self.location
            .as_ref()
            .map(|src| (src.path.as_ref().to_path_buf(), src.location.clone()))
    }

    /// Return the file content. Or `None` if there is no such information.
    pub fn file_content(&self) -> Option<Text> {
        self.location.as_ref().map(|src| src.content.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wants_impl(_: impl Config) {}

    #[test]
    fn test_btreemap_config() {
        let map: BTreeMap<&str, &str> = vec![("foo.bar", "baz")].into_iter().collect();
        assert_eq!(format!("{:?}", Config::keys(&map, "foo")), "[\"bar\"]");
        assert_eq!(
            format!("{:?}", Config::get(&map, "foo", "bar")),
            "Some(\"baz\")"
        );
        assert_eq!(format!("{:?}", Config::get(&map, "foo", "baz")), "None");

        // Make sure we can pass BTreeMap config to generic func.
        wants_impl(&map);
    }
}
