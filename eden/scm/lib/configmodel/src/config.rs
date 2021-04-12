/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::convert::FromConfigValue;
use crate::Result;
use minibytes::Text;
use std::str;

/// Readable config. This can be used as a trait object.
pub trait Config {
    /// Get config names in the given section. Sorted by insertion order.
    fn keys(&self, section: &str) -> Vec<Text>;

    /// Get config value for a given config.
    /// Return `None` if the config item does not exist or is unset.
    fn get(&self, section: &str, name: &str) -> Option<Text>;
}

/// Extra APIs (incompatible with trait objects) around reading config.
pub trait ConfigExt: Config {
    /// Get a config item. Convert to type `T`.
    fn get_opt<T: FromConfigValue>(&self, section: &str, name: &str) -> Result<Option<T>> {
        self.get(section, name)
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

impl Config for &dyn Config {
    fn keys(&self, section: &str) -> Vec<Text> {
        (*self).keys(section)
    }

    fn get(&self, section: &str, name: &str) -> Option<Text> {
        (*self).get(section, name)
    }
}
