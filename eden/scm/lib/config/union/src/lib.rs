/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::Arc;

use configmodel::Config;
use configmodel::Text;
use configmodel::ValueSource;
use indexmap::IndexSet;

/// A union of multiple configs.
#[derive(Clone)]
pub struct UnionConfig {
    configs: Vec<Arc<dyn Config>>,
    name: Text,
}

impl Default for UnionConfig {
    fn default() -> Self {
        Self::from_configs(Vec::new())
    }
}

impl UnionConfig {
    /// Construct `UnionConfig` from a list of configs.
    /// In case of conflicts, the last one wins.
    pub fn from_configs(configs: Vec<Arc<dyn Config>>) -> Self {
        Self {
            configs,
            name: Text::from_static("UnionConfig"),
        }
    }

    /// Assign a name to this config.
    pub fn named(self, name: Text) -> Self {
        Self {
            configs: self.configs,
            name,
        }
    }

    /// Push a config as a layer. It overrides other configs.
    pub fn push(&mut self, config: Arc<dyn Config>) {
        self.configs.push(config)
    }
}

impl Config for UnionConfig {
    fn keys(&self, section: &str) -> Vec<Text> {
        let mut result: IndexSet<Text> = Default::default();
        // normal order: match order loading configs
        for config in self.configs.iter() {
            for key in config.keys(section) {
                result.insert(key);
            }
        }
        result.into_iter().collect()
    }

    fn get_considering_unset(&self, section: &str, name: &str) -> Option<Option<Text>> {
        // rev order: last config counts
        for config in self.configs.iter().rev() {
            if let Some(value) = config.get_considering_unset(section, name) {
                return Some(value);
            }
        }
        None
    }

    fn sections(&self) -> Cow<[Text]> {
        let mut result: IndexSet<Text> = Default::default();
        // normal order: match order loading configs
        for config in self.configs.iter() {
            for section in config.sections().iter() {
                result.insert(section.clone());
            }
        }
        let result: Vec<Text> = result.into_iter().collect();
        result.into()
    }

    fn get_sources(&self, section: &str, name: &str) -> Cow<[ValueSource]> {
        // The last "Source" counts.
        // normal order: match order loading configs
        let mut result = Vec::new();
        for config in self.configs.iter() {
            let mut sources = config.get_sources(section, name).into_owned();
            result.append(&mut sources);
        }
        result.into()
    }

    fn files(&self) -> Cow<[PathBuf]> {
        let mut result: IndexSet<PathBuf> = Default::default();
        // normal order: match order loading configs
        for config in self.configs.iter() {
            for path in config.files().into_owned() {
                result.insert(path);
            }
        }
        let result: Vec<PathBuf> = result.into_iter().collect();
        result.into()
    }

    fn layers(&self) -> Vec<Arc<dyn Config>> {
        self.configs.clone()
    }

    fn layer_name(&self) -> Text {
        self.name.clone()
    }
}

#[cfg(test)]
mod tests {
    use staticconfig::static_config;

    use super::*;

    #[test]
    fn test_basic() {
        let config1 = static_config!(
            r#"
[b]
w=1
v=2

[a]
v=3
w=5

[d]
v=4
"#
        )
        .named("1.rc");
        let config2 = static_config!(
            r#"
[c]
v=11

[b]
z=12
v=13
"#
        )
        .named("2.rc");
        let config3 = static_config!(
            r#"
[a]
v=21
%unset w

[c]
v=22

[e]
v=23
"#
        )
        .named("3.rc");
        let config = UnionConfig::from_configs(vec![
            Arc::new(config1),
            Arc::new(config2),
            Arc::new(config3),
        ])
        .named("unioned".into());

        // sections() preserves order.
        assert_eq!(config.sections().into_owned(), ["b", "a", "d", "c", "e"]);

        // keys() preserves order.
        assert_eq!(config.keys("b"), ["w", "v", "z"]);

        // get(): last config wins.
        assert_eq!(config.get("a", "v").unwrap(), "21");
        assert_eq!(config.get("a", "w"), None);
        assert_eq!(config.get("b", "v").unwrap(), "13");
        assert_eq!(config.get("b", "w").unwrap(), "1");
        assert_eq!(config.get("c", "v").unwrap(), "22");
        assert_eq!(config.get("d", "v").unwrap(), "4");
        assert_eq!(config.get("e", "v").unwrap(), "23");

        // get_considering_unset()
        assert_eq!(config.get_considering_unset("a", "w"), Some(None));
        assert_eq!(config.get_considering_unset("a", "x"), None);

        // get_sources() returns all sources. The last one counts.
        assert_eq!(
            config
                .get_sources("a", "v")
                .iter()
                .map(|s| s.source.to_string())
                .collect::<Vec<_>>(),
            ["1.rc", "3.rc"]
        );

        // layers() returns configs in order.
        assert_eq!(
            config
                .layers()
                .into_iter()
                .map(|c| c.layer_name())
                .collect::<Vec<_>>(),
            ["1.rc", "2.rc", "3.rc"]
        );

        // layer_name()
        assert_eq!(config.layer_name(), "unioned");
    }

    #[test]
    fn test_push() {
        let config1 = static_config! {"b": { "v": "1" }};
        let config2 = static_config! {"b": { "v": "2" }};

        let mut config = UnionConfig::default();
        config.push(Arc::new(config1));
        assert_eq!(config.get("b", "v").unwrap(), "1");
        config.push(Arc::new(config2));
        assert_eq!(config.get("b", "v").unwrap(), "2");
    }
}
