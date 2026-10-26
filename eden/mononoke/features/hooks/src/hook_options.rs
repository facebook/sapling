/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Result;
use metaconfig_types::HookConfig;
use serde::de::DeserializeOwned;
use stats::prelude::*;

define_stats! {
    prefix = "mononoke.hooks";
    typed_config_used: dynamic_timeseries("{}.typed_config_used", (implementation: &'static str); Rate, Sum),
}

/// Parses the options of a hook of the given implementation.
///
/// When the hook config sets `config_json`, the options are deserialized from it and the typed
/// config maps are ignored. Otherwise `from_typed` builds the options from the typed config maps.
/// A config that sets neither is also passed to `from_typed`, which must then produce the same
/// result as a `config_json` of `{}`.
pub(crate) fn parse_hook_options<T>(
    config: &HookConfig,
    implementation: &'static str,
    from_typed: impl FnOnce(&HookConfig) -> Result<T>,
) -> Result<T>
where
    T: DeserializeOwned,
{
    if let Some(options) = &config.options {
        return serde_json::from_str(options)
            .with_context(|| format!("Invalid config_json for {implementation} hook"));
    }
    if has_typed_options(config) {
        STATS::typed_config_used.add_value(1, (implementation,));
    }
    from_typed(config)
}

fn has_typed_options(config: &HookConfig) -> bool {
    !config.strings.is_empty()
        || !config.ints.is_empty()
        || !config.ints_64.is_empty()
        || !config.string_lists.is_empty()
        || !config.int_lists.is_empty()
        || !config.int_64_lists.is_empty()
}

#[cfg(test)]
mod tests {
    use anyhow::anyhow;
    use mononoke_macros::mononoke;
    use serde::Deserialize;

    use super::*;

    #[derive(Debug, Default, Deserialize, PartialEq)]
    #[serde(default)]
    struct TestOptions {
        value: Option<String>,
    }

    fn from_typed(config: &HookConfig) -> Result<TestOptions> {
        Ok(TestOptions {
            value: config.strings.get("value").cloned(),
        })
    }

    fn typed_config(value: &str) -> HookConfig {
        HookConfig {
            strings: [("value".to_owned(), value.to_owned())].into(),
            ..Default::default()
        }
    }

    #[mononoke::test]
    fn test_config_json_wins_over_typed_options() -> Result<()> {
        let config = HookConfig {
            options: Some(r#"{"value": "json"}"#.to_owned()),
            ..typed_config("typed")
        };
        let options: TestOptions = parse_hook_options(&config, "test", from_typed)?;
        assert_eq!(options.value.as_deref(), Some("json"));
        Ok(())
    }

    #[mononoke::test]
    fn test_typed_options_used_without_config_json() -> Result<()> {
        let options: TestOptions = parse_hook_options(&typed_config("typed"), "test", from_typed)?;
        assert_eq!(options.value.as_deref(), Some("typed"));
        Ok(())
    }

    #[mononoke::test]
    fn test_empty_config_uses_from_typed() -> Result<()> {
        let result: Result<TestOptions> =
            parse_hook_options(&HookConfig::default(), "test", |_| Err(anyhow!("typed")));
        assert_eq!(result.unwrap_err().to_string(), "typed");
        Ok(())
    }

    #[mononoke::test]
    fn test_invalid_config_json_is_an_error() {
        let config = HookConfig {
            options: Some("not json".to_owned()),
            ..Default::default()
        };
        let result: Result<TestOptions> = parse_hook_options(&config, "test", from_typed);
        assert!(result.is_err());
    }
}
