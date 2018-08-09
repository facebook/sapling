//! Mercurial-specific config postprocessing

use bytes::Bytes;
use config::{ConfigSet, Options};
use std::collections::HashSet;
use std::env;
use std::path::Path;

const HGPLAIN: &str = "HGPLAIN";
const HGPLAINEXCEPT: &str = "HGPLAINEXCEPT";

pub trait OptionsHgExt {
    /// Drop configs according to HGPLAIN and HGPLAINEXCEPT.
    fn process_hgplain(self) -> Self;
}

pub trait ConfigSetHgExt {
    /// Load system config files.
    /// `data_dir` is `mercurial.util.datapath`.
    fn load_system<P: AsRef<Path>>(&mut self, data_dir: P);

    /// Load user config files (and environment variables).
    fn load_user(&mut self);
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
}

impl ConfigSetHgExt for ConfigSet {
    fn load_system<P: AsRef<Path>>(&mut self, data_dir: P) {
        let opts = Options::new().source("system").process_hgplain();
        let data_dir = data_dir.as_ref();

        if env::var("HGRCPATH").is_err() {
            #[cfg(unix)]
            {
                self.load_path(data_dir.join("default.d/"), &opts);
                self.load_path("/etc/mercurial/hgrc", &opts);
                self.load_path("/etc/mercurial/hgrc.d/", &opts);
            }

            #[cfg(windows)]
            {
                self.load_path(data_dir.join("default.d/"), &opts);
                self.load_path(data_dir.join("mercurial.ini"), &opts);
                self.load_path(data_dir.join("hgrc.d/"), &opts);
            }
        }
    }

    fn load_user(&mut self) {
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

        let opts = Options::new().source("user").process_hgplain();

        // If $HGRCPATH is set, use it instead.
        if let Ok(rcpath) = env::var("HGRCPATH") {
            #[cfg(unix)]
            let paths = rcpath.split(':');
            #[cfg(windows)]
            let paths = rcpath.split(';');
            for path in paths {
                self.load_path(path, &opts);
            }
        } else {
            #[allow(deprecated)]
            let option_home_dir = env::home_dir();
            if let Some(home_dir) = option_home_dir {
                #[cfg(unix)]
                {
                    self.load_path(home_dir.join(".hgrc"), &opts);
                    if let Ok(config_home) = ::std::env::var("XDG_CONFIG_HOME") {
                        self.load_path(format!("{}/hg/hgrc", config_home), &opts);
                    }
                }

                #[cfg(windows)]
                {
                    self.load_path(home_dir.join("mercurial.ini"), &opts);
                    self.load_path(home_dir.join(".hgrc"), &opts);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
