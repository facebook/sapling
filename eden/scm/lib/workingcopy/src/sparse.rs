/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use configmodel::Config;
use configmodel::ConfigExt;

fn config_overrides(config: &dyn configmodel::Config) -> HashMap<String, String> {
    let mut overrides: HashMap<String, String> = HashMap::new();
    for key in config.keys("sparseprofile") {
        let parts: Vec<&str> = key.splitn(3, '.').collect();
        if parts.len() != 3 {
            tracing::warn!(?key, "invalid sparseprofile config key");
            continue;
        }

        let (sparse_section, prof_name) = (parts[0], parts[2]);

        let vals = match config.get_or_default::<Vec<String>>("sparseprofile", &key) {
            Ok(vals) => vals,
            Err(err) => {
                tracing::warn!(?key, ?err, "invalid sparseprofile config value");
                continue;
            }
        };

        overrides
            .entry(prof_name.into())
            .or_default()
            .push_str(&format!(
                "\n# source = hgrc.dynamic \"{}\"\n[{}]\n{}\n# source =\n",
                key,
                sparse_section,
                vals.join("\n")
            ));
    }

    overrides
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    #[test]
    fn test_config_overrides() {
        let mut conf = BTreeMap::new();

        conf.insert("sparseprofile.include.foo.someprof", "inca,incb");
        conf.insert("sparseprofile.include.bar.someprof", "incc");
        conf.insert("sparseprofile.exclude.foo.someprof", "exca,excb");

        conf.insert("sparseprofile.include.foo.otherprof", "inca");

        assert_eq!(
            config_overrides(&conf),
            HashMap::from([
                (
                    "someprof".to_string(),
                    r#"
# source = hgrc.dynamic "exclude.foo.someprof"
[exclude]
exca
excb
# source =

# source = hgrc.dynamic "include.bar.someprof"
[include]
incc
# source =

# source = hgrc.dynamic "include.foo.someprof"
[include]
inca
incb
# source =
"#
                    .to_string()
                ),
                (
                    "otherprof".to_string(),
                    r#"
# source = hgrc.dynamic "include.foo.otherprof"
[include]
inca
# source =
"#
                    .to_string()
                ),
            ])
        );
    }
}
