// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.
use bytes::Bytes;
use configparser::config::ConfigSet;
use configparser::hg::ConfigSetHgExt;
use std::path::Path;

fn load_config() -> ConfigSet {
    // priority is ->
    //     - system
    //     - user
    //     - repo
    //     - configfile
    //     - config ( bottom overrides above )
    let mut errors = Vec::new();
    let mut config = ConfigSet::new();
    errors.extend(config.load_system());
    errors.extend(config.load_user());
    // config.load_repo(); // TODO:  implement in configparser.rs
    // TODO: errors are ignored and should probably return a Fallible
    config
}

fn override_config(
    mut config: ConfigSet,
    config_paths: &[&Path],
    config_overrides: &[&str],
) -> ConfigSet {
    let mut errors = Vec::new();

    for config_path in config_paths {
        errors.extend(config.load_path(config_path, &"--configfile".into()));
    }

    // TODO:  should not panic on incorrect format of --config flag, should eventually return errors
    for config_override in config_overrides {
        let equals_pos = config_override.find("=").unwrap();
        let section_name_pair = &config_override[..equals_pos];
        let value = &config_override[equals_pos + 1..];

        let dot_pos = section_name_pair.find(".").unwrap();
        let section = &section_name_pair[..dot_pos];
        let name = &section_name_pair[dot_pos + 1..];

        config.set(section, name, Some(&Bytes::from(value)), &"--config".into());
    }

    config
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use std::path::PathBuf;
    use tempfile::tempdir;

    #[test]
    fn test_config_with_config_overrides_present() {
        let config = ConfigSet::new();
        let config_pairs = vec!["foo.bar=1", "bar.foo=2"];
        let config = override_config(config, &[], &config_pairs);

        assert_eq!(
            config.sections(),
            vec![Bytes::from("foo"), Bytes::from("bar")]
        );
        assert_eq!(config.keys("foo"), vec![Bytes::from("bar")]);
        assert_eq!(config.keys("bar"), vec![Bytes::from("foo")]);

        assert_eq!(config.get("foo", "bar"), Some(Bytes::from("1")));
        assert_eq!(config.get("bar", "foo"), Some(Bytes::from("2")));

        let sources = config.get_sources("foo", "bar");
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].source(), &Bytes::from("--config"));
    }

    #[test]
    fn test_config_with_complex_value() {
        let config = ConfigSet::new();
        let config_pairs = vec!["pager.pager=LESS=FRKX less"];
        let config = override_config(config, &[], &config_pairs);

        assert_eq!(config.sections(), vec![Bytes::from("pager")]);

        assert_eq!(config.keys("pager"), vec![Bytes::from("pager")]);
        assert_eq!(
            config.get("pager", "pager"),
            Some(Bytes::from("LESS=FRKX less"))
        );

        let sources = config.get_sources("pager", "pager");
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].source(), &Bytes::from("--config"));
    }

    pub(crate) fn write_file(path: PathBuf, content: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut f = fs::File::create(path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
    }

    #[test]
    fn test_config_with_configfile_overrides_present() {
        let dir = tempdir().unwrap();

        let config = ConfigSet::new();
        write_file(dir.path().join("foo.rc"), "[foo]\nbar=1\n[bar]\nfoo=2");
        let path = dir.path().join("foo.rc");
        let configfiles = vec![path.as_path()];
        let config = override_config(config, &configfiles, &[]);

        assert_eq!(
            config.sections(),
            vec![Bytes::from("foo"), Bytes::from("bar")]
        );
        assert_eq!(config.keys("foo"), vec![Bytes::from("bar")]);
        assert_eq!(config.keys("bar"), vec![Bytes::from("foo")]);

        assert_eq!(config.get("foo", "bar"), Some(Bytes::from("1")));
        assert_eq!(config.get("bar", "foo"), Some(Bytes::from("2")));

        let sources = config.get_sources("foo", "bar");
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].source(), &Bytes::from("--configfile"));
    }

}
