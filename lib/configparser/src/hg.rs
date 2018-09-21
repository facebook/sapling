//! Mercurial-specific config postprocessing

use bytes::Bytes;
use config::{expand_path, ConfigSet, Options};
use dirs;
use error::Error;
use std::cmp::Eq;
use std::collections::{HashMap, HashSet};
use std::env;
use std::hash::Hash;
use std::path::Path;

const HGPLAIN: &str = "HGPLAIN";
const HGPLAINEXCEPT: &str = "HGPLAINEXCEPT";
const HGRCPATH: &str = "HGRCPATH";

pub trait OptionsHgExt {
    /// Drop configs according to `$HGPLAIN` and `$HGPLAINEXCEPT`.
    fn process_hgplain(self) -> Self;

    /// Set read-only config items. `items` contains a list of tuple `(section, name)`.
    /// Setting those items to new value will be ignored.
    fn readonly_items<S: Into<Bytes>, N: Into<Bytes>>(self, items: Vec<(S, N)>) -> Self;

    /// Set section remap. If a section name matches an entry key, it will be treated as if the
    /// name is the entry value. The remap wouldn't happen recursively. For example, with a
    /// `{"A": "B", "B": "C"}` map, section name "A" will be treated as "B", not "C".
    /// This is implemented via `append_filter`.
    fn remap_sections<K: Eq + Hash + Into<Bytes>, V: Into<Bytes>>(
        self,
        remap: HashMap<K, V>,
    ) -> Self;

    /// Set section whitelist. Sections outside the whitelist won't be loaded.
    /// This is implemented via `append_filter`.
    fn whitelist_sections<B: Clone + Into<Bytes>>(self, sections: Vec<B>) -> Self;
}

pub trait ConfigSetHgExt {
    /// Load system config files if `$HGRCPATH` is not set.
    /// `data_dir` is `mercurial.util.datapath`.
    /// Return errors parsing files.
    fn load_system<P: AsRef<Path>>(&mut self, data_dir: P) -> Vec<Error>;

    /// Load user config files (and environment variables).  If `$HGRCPATH` is
    /// set, load files listed in that environment variable instead.
    /// Return errors parsing files.
    fn load_user(&mut self) -> Vec<Error>;
}

impl OptionsHgExt for Options {
    fn process_hgplain(self) -> Self {
        let plain_set = env::var(HGPLAIN).is_ok();
        let plain_except = env::var(HGPLAINEXCEPT);
        if plain_set || plain_except.is_ok() {
            let (section_blacklist, ui_blacklist) = {
                let plain_exceptions: HashSet<String> = plain_except
                    .unwrap_or_else(|_| "".to_string())
                    .split(',')
                    .map(|s| s.to_string())
                    .collect();

                // [defaults] and [commands] are always blacklisted.
                let mut section_blacklist: HashSet<Bytes> =
                    ["defaults", "commands"].iter().map(|&s| s.into()).collect();

                // [alias], [revsetalias], [templatealias] are blacklisted if they are outside
                // HGPLAINEXCEPT.
                for &name in ["alias", "revsetalias", "templatealias"].iter() {
                    if !plain_exceptions.contains(name) {
                        section_blacklist.insert(Bytes::from(name));
                    }
                }

                // These configs under [ui] are always blacklisted.
                let mut ui_blacklist: HashSet<Bytes> = [
                    "debug",
                    "fallbackencoding",
                    "quiet",
                    "slash",
                    "logtemplate",
                    "statuscopies",
                    "style",
                    "traceback",
                    "verbose",
                ].iter()
                    .map(|&s| s.into())
                    .collect();
                // exitcodemask is blacklisted if exitcode is outside HGPLAINEXCEPT.
                if !plain_exceptions.contains("exitcode") {
                    ui_blacklist.insert("exitcodemask".into());
                }

                (section_blacklist, ui_blacklist)
            };

            let filter = move |section: Bytes, name: Bytes, value: Option<Bytes>| {
                if section_blacklist.contains(&section)
                    || (section.as_ref() == b"ui" && ui_blacklist.contains(&name))
                {
                    None
                } else {
                    Some((section, name, value))
                }
            };

            self.append_filter(Box::new(filter))
        } else {
            self
        }
    }

    /// Set section whitelist. Sections outside the whitelist won't be loaded.
    /// This is implemented via `append_filter`.
    fn whitelist_sections<B: Clone + Into<Bytes>>(self, sections: Vec<B>) -> Self {
        let whitelist: HashSet<Bytes> = sections
            .iter()
            .cloned()
            .map(|section| section.into())
            .collect();

        let filter = move |section: Bytes, name: Bytes, value: Option<Bytes>| {
            if whitelist.contains(&section) {
                Some((section, name, value))
            } else {
                None
            }
        };

        self.append_filter(Box::new(filter))
    }

    /// Set section remap. If a section name matches an entry key, it will be treated as if the
    /// name is the entry value. The remap wouldn't happen recursively. For example, with a
    /// `{"A": "B", "B": "C"}` map, section name "A" will be treated as "B", not "C".
    /// This is implemented via `append_filter`.
    fn remap_sections<K, V>(self, remap: HashMap<K, V>) -> Self
    where
        K: Eq + Hash + Into<Bytes>,
        V: Into<Bytes>,
    {
        let remap: HashMap<Bytes, Bytes> = remap
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect();

        let filter = move |section: Bytes, name: Bytes, value: Option<Bytes>| {
            let section = remap.get(&section).cloned().unwrap_or(section);
            Some((section, name, value))
        };

        self.append_filter(Box::new(filter))
    }

    fn readonly_items<S: Into<Bytes>, N: Into<Bytes>>(self, items: Vec<(S, N)>) -> Self {
        let readonly_items: HashSet<(Bytes, Bytes)> = items
            .into_iter()
            .map(|(section, name)| (section.into(), name.into()))
            .collect();

        let filter = move |section: Bytes, name: Bytes, value: Option<Bytes>| {
            if readonly_items.contains(&(section.clone(), name.clone())) {
                None
            } else {
                Some((section, name, value))
            }
        };

        self.append_filter(Box::new(filter))
    }
}

impl ConfigSetHgExt for ConfigSet {
    fn load_system<P: AsRef<Path>>(&mut self, data_dir: P) -> Vec<Error> {
        let opts = Options::new().source("system").process_hgplain();
        let data_dir = data_dir.as_ref();
        let mut errors = Vec::new();

        if env::var(HGRCPATH).is_err() {
            #[cfg(unix)]
            {
                errors.append(&mut self.load_path(data_dir.join("default.d/"), &opts));
                errors.append(&mut self.load_path("/etc/mercurial/hgrc", &opts));
                errors.append(&mut self.load_path("/etc/mercurial/hgrc.d/", &opts));
            }

            #[cfg(windows)]
            {
                let exe =
                    env::current_exe().expect("abort: could not fetch the current executable");
                let exe_dir = exe.parent().unwrap();
                errors.append(&mut self.load_path(data_dir.join("default.d/"), &opts));
                errors.append(&mut self.load_path(exe_dir.join("mercurial.ini"), &opts));
                errors.append(&mut self.load_path(exe_dir.join("hgrc.d/"), &opts));
            }
        }

        errors
    }

    fn load_user(&mut self) -> Vec<Error> {
        let mut errors = Vec::new();

        // Covert "$VISUAL", "$EDITOR" to "ui.editor".
        //
        // Unlike Mercurial, don't convert the "$PAGER" environment variable
        // to "pager.pager" config.
        //
        // The environment variable could be from the system profile (ex.
        // /etc/profile.d/...), or the user shell rc (ex. ~/.bashrc). There is
        // no clean way to tell which one it is from.  The value might be
        // tweaked for sysadmin usecases (ex. -n), which are different from
        // SCM's usecases.
        for name in ["VISUAL", "EDITOR"].iter() {
            if let Ok(editor) = env::var(name) {
                self.set(
                    "ui",
                    "editor",
                    Some(editor.as_bytes()),
                    &Options::new().source(format!("${}", name)),
                );
                break;
            }
        }

        // Convert $HGPROF to profiling.type
        if let Ok(profiling_type) = env::var("HGPROF") {
            self.set(
                "profiling",
                "type",
                Some(profiling_type.as_bytes()),
                &"$HGPROF".into(),
            );
        }

        let opts = Options::new().source("user").process_hgplain();

        // If $HGRCPATH is set, use it instead.
        if let Ok(rcpath) = env::var("HGRCPATH") {
            #[cfg(unix)]
            let paths = rcpath.split(':');
            #[cfg(windows)]
            let paths = rcpath.split(';');
            for path in paths {
                errors.append(&mut self.load_path(expand_path(path), &opts));
            }
        } else {
            let option_home_dir = dirs::home_dir();
            if let Some(home_dir) = option_home_dir {
                #[cfg(unix)]
                {
                    errors.append(&mut self.load_path(home_dir.join(".hgrc"), &opts));
                    if let Ok(config_home) = ::std::env::var("XDG_CONFIG_HOME") {
                        errors
                            .append(&mut self.load_path(format!("{}/hg/hgrc", config_home), &opts));
                    }
                }

                #[cfg(windows)]
                {
                    errors.append(&mut self.load_path(home_dir.join("mercurial.ini"), &opts));
                    errors.append(&mut self.load_path(home_dir.join(".hgrc"), &opts));
                }
            }
        }

        errors
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use config::tests::write_file;
    use tempdir::TempDir;

    #[test]
    fn test_basic_hgplain() {
        env::set_var(HGPLAIN, "1");
        env::remove_var(HGPLAINEXCEPT);

        let opts = Options::new().process_hgplain();
        let mut cfg = ConfigSet::new();
        cfg.parse(
            "[defaults]\n\
             commit = commit -d 0\n\
             [ui]\n\
             verbose = true\n\
             username = test\n\
             [alias]\n\
             l = log\n",
            &opts,
        );

        assert!(cfg.keys("defaults").is_empty());
        assert_eq!(cfg.get("ui", "verbose"), None);
        assert_eq!(cfg.get("ui", "username"), Some("test".into()));
        assert_eq!(cfg.get("alias", "l"), None);
    }

    #[test]
    fn test_hgplainexcept() {
        env::remove_var(HGPLAIN);
        env::set_var(HGPLAINEXCEPT, "alias,revsetalias");

        let opts = Options::new().process_hgplain();
        let mut cfg = ConfigSet::new();
        cfg.parse(
            "[defaults]\n\
             commit = commit -d 0\n\
             [alias]\n\
             l = log\n\
             [templatealias]\n\
             u = user\n\
             [revsetalias]\n\
             @ = master\n",
            &opts,
        );

        assert!(cfg.keys("defaults").is_empty());
        assert_eq!(cfg.get("alias", "l"), Some("log".into()));
        assert_eq!(cfg.get("revsetalias", "@"), Some("master".into()));
        assert_eq!(cfg.get("templatealias", "u"), None);
    }

    #[test]
    fn test_hgrcpath() {
        let dir = TempDir::new("test_hgrcpath").unwrap();

        write_file(dir.path().join("1.rc"), "[x]\na=1");
        write_file(dir.path().join("2.rc"), "[y]\nb=2");

        #[cfg(unix)]
        let hgrcpath = "$T/1.rc:$T/2.rc";
        #[cfg(windows)]
        let hgrcpath = "$T/1.rc;%T%/2.rc";

        env::set_var("T", dir.path());
        env::set_var(HGRCPATH, hgrcpath);

        let mut cfg = ConfigSet::new();

        cfg.load_system("");
        assert!(cfg.sections().is_empty());

        cfg.load_user();
        assert_eq!(cfg.get("x", "a"), Some("1".into()));
        assert_eq!(cfg.get("y", "b"), Some("2".into()));
    }

    #[test]
    fn test_section_whitelist() {
        let opts = Options::new().whitelist_sections(vec!["x", "y"]);
        let mut cfg = ConfigSet::new();
        cfg.parse(
            "[x]\n\
             a=1\n\
             [y]\n\
             b=2\n\
             [z]\n\
             c=3",
            &opts,
        );

        assert_eq!(cfg.sections(), vec![Bytes::from("x"), Bytes::from("y")]);
        assert_eq!(cfg.get("z", "c"), None);
    }

    #[test]
    fn test_section_remap() {
        let mut remap = HashMap::new();
        remap.insert("x", "y");
        remap.insert("y", "z");

        let opts = Options::new().remap_sections(remap);
        let mut cfg = ConfigSet::new();
        cfg.parse(
            "[x]\n\
             a=1\n\
             [y]\n\
             b=2\n\
             [z]\n\
             c=3",
            &opts,
        );

        assert_eq!(cfg.get("y", "a"), Some("1".into()));
        assert_eq!(cfg.get("z", "b"), Some("2".into()));
        assert_eq!(cfg.get("z", "c"), Some("3".into()));
    }

    #[test]
    fn test_readonly_items() {
        let opts = Options::new().readonly_items(vec![("x", "a"), ("y", "b")]);
        let mut cfg = ConfigSet::new();
        cfg.parse(
            "[x]\n\
             a=1\n\
             [y]\n\
             b=2\n\
             [z]\n\
             c=3",
            &opts,
        );

        assert_eq!(cfg.get("x", "a"), None);
        assert_eq!(cfg.get("y", "b"), None);
        assert_eq!(cfg.get("z", "c"), Some("3".into()));
    }
}
