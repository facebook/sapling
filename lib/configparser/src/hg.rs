//! Mercurial-specific config postprocessing

use bytes::Bytes;
use config::Options;
use std::collections::HashSet;
use std::env;

const HGPLAIN: &str = "HGPLAIN";
const HGPLAINEXCEPT: &str = "HGPLAINEXCEPT";

pub trait OptionsHgExt {
    /// Drop configs according to HGPLAIN and HGPLAINEXCEPT.
    fn process_hgplain(self) -> Self;
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

#[cfg(test)]
mod tests {
    use super::*;
    use config::ConfigSet;

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
