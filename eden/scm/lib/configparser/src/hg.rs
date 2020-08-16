/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Mercurial-specific config postprocessing

use std::cmp::Eq;
use std::collections::{HashMap, HashSet};
use std::env;
use std::hash::Hash;
use std::path::{Path, PathBuf};

use anyhow::Result;
use minibytes::Text;
use util::path::expand_path;

use crate::config::{ConfigSet, Options};
use crate::error::{Error, Errors};

pub const HGPLAIN: &str = "HGPLAIN";
pub const HGPLAINEXCEPT: &str = "HGPLAINEXCEPT";
pub const HGRCPATH: &str = "HGRCPATH";

pub trait OptionsHgExt {
    /// Drop configs according to `$HGPLAIN` and `$HGPLAINEXCEPT`.
    fn process_hgplain(self) -> Self;

    /// Set read-only config items. `items` contains a list of tuple `(section, name)`.
    /// Setting those items to new value will be ignored.
    fn readonly_items<S: Into<Text>, N: Into<Text>>(self, items: Vec<(S, N)>) -> Self;

    /// Set section remap. If a section name matches an entry key, it will be treated as if the
    /// name is the entry value. The remap wouldn't happen recursively. For example, with a
    /// `{"A": "B", "B": "C"}` map, section name "A" will be treated as "B", not "C".
    /// This is implemented via `append_filter`.
    fn remap_sections<K: Eq + Hash + Into<Text>, V: Into<Text>>(self, remap: HashMap<K, V>)
        -> Self;

    /// Filter sections. Sections outside include_sections won't be loaded.
    /// This is implemented via `append_filter`.
    fn filter_sections<B: Clone + Into<Text>>(self, include_sections: Vec<B>) -> Self;
}

pub trait ConfigSetHgExt {
    /// Load system config files if `$HGRCPATH` is not set.
    /// Return errors parsing files.
    fn load_system(&mut self) -> Vec<Error>;

    /// Load user config files (and environment variables).  If `$HGRCPATH` is
    /// set, load files listed in that environment variable instead.
    /// Return errors parsing files.
    fn load_user(&mut self) -> Vec<Error>;

    /// Load a specified config file. Respect HGPLAIN environment variables.
    /// Return errors parsing files.
    fn load_hgrc(&mut self, path: impl AsRef<Path>, source: &'static str) -> Vec<Error>;

    /// Get a config item. Convert to type `T`.
    fn get_opt<T: FromConfigValue>(&self, section: &str, name: &str) -> Result<Option<T>>;

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

pub trait FromConfigValue: Sized {
    fn try_from_str(s: &str) -> Result<Self>;
}

/// Load system, user config files.
pub fn load() -> Result<ConfigSet> {
    let mut set = ConfigSet::new();
    let mut errors = vec![];

    // Only load builtin configs if HGRCPATH is not set.
    if std::env::var(HGRCPATH).is_err() {
        errors.append(&mut set.parse(MERGE_TOOLS_CONFIG, &"merge-tools.rc".into()));
    }
    errors.append(&mut set.load_system());
    errors.append(&mut set.load_user());

    if !errors.is_empty() {
        return Err(Errors(errors).into());
    }
    Ok(set)
}

impl OptionsHgExt for Options {
    fn process_hgplain(self) -> Self {
        let plain_set = env::var(HGPLAIN).is_ok();
        let plain_except = env::var(HGPLAINEXCEPT);
        if plain_set || plain_except.is_ok() {
            let (section_exclude_list, ui_exclude_list) = {
                let plain_exceptions: HashSet<String> = plain_except
                    .unwrap_or_else(|_| "".to_string())
                    .split(',')
                    .map(|s| s.to_string())
                    .collect();

                // [defaults] and [commands] are always excluded.
                let mut section_exclude_list: HashSet<Text> =
                    ["defaults", "commands"].iter().map(|&s| s.into()).collect();

                // [alias], [revsetalias], [templatealias] are excluded if they are outside
                // HGPLAINEXCEPT.
                for &name in ["alias", "revsetalias", "templatealias"].iter() {
                    if !plain_exceptions.contains(name) {
                        section_exclude_list.insert(Text::from(name));
                    }
                }

                // These configs under [ui] are always excluded.
                let mut ui_exclude_list: HashSet<Text> = [
                    "debug",
                    "fallbackencoding",
                    "quiet",
                    "slash",
                    "logtemplate",
                    "statuscopies",
                    "style",
                    "traceback",
                    "verbose",
                ]
                .iter()
                .map(|&s| s.into())
                .collect();
                // exitcodemask is excluded if exitcode is outside HGPLAINEXCEPT.
                if !plain_exceptions.contains("exitcode") {
                    ui_exclude_list.insert("exitcodemask".into());
                }

                (section_exclude_list, ui_exclude_list)
            };

            let filter = move |section: Text, name: Text, value: Option<Text>| {
                if section_exclude_list.contains(&section)
                    || (section.as_ref() == "ui" && ui_exclude_list.contains(&name))
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

    /// Filter sections. Sections outside of include_sections won't be loaded.
    /// This is implemented via `append_filter`.
    fn filter_sections<B: Clone + Into<Text>>(self, include_sections: Vec<B>) -> Self {
        let include_list: HashSet<Text> = include_sections
            .iter()
            .cloned()
            .map(|section| section.into())
            .collect();

        let filter = move |section: Text, name: Text, value: Option<Text>| {
            if include_list.contains(&section) {
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
        K: Eq + Hash + Into<Text>,
        V: Into<Text>,
    {
        let remap: HashMap<Text, Text> = remap
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect();

        let filter = move |section: Text, name: Text, value: Option<Text>| {
            let section = remap.get(&section).cloned().unwrap_or(section);
            Some((section, name, value))
        };

        self.append_filter(Box::new(filter))
    }

    fn readonly_items<S: Into<Text>, N: Into<Text>>(self, items: Vec<(S, N)>) -> Self {
        let readonly_items: HashSet<(Text, Text)> = items
            .into_iter()
            .map(|(section, name)| (section.into(), name.into()))
            .collect();

        let filter = move |section: Text, name: Text, value: Option<Text>| {
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
    fn load_system(&mut self) -> Vec<Error> {
        let opts = Options::new().source("system").process_hgplain();
        let mut errors = Vec::new();

        if env::var(HGRCPATH).is_err() {
            #[cfg(unix)]
            {
                errors.append(&mut self.load_path("/etc/mercurial/system.rc", &opts));
                // TODO(T40519286): Remove this after the tupperware overrides move out of hgrc.d
                errors.append(
                    &mut self.load_path("/etc/mercurial/hgrc.d/tupperware_overrides.rc", &opts),
                );
                // TODO(quark): Remove this after packages using system.rc are rolled out
                errors.append(&mut self.load_path("/etc/mercurial/hgrc.d/include.rc", &opts));
            }

            #[cfg(windows)]
            {
                if let Ok(program_data_path) = env::var("PROGRAMDATA") {
                    let hgrc_dir = Path::new(&program_data_path).join("Facebook\\Mercurial");
                    errors.append(&mut self.load_path(hgrc_dir.join("system.rc"), &opts));
                    // TODO(quark): Remove this after packages using system.rc are rolled out
                    errors.append(&mut self.load_path(hgrc_dir.join("hgrc"), &opts));
                }
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
                    Some(editor),
                    &Options::new().source(format!("${}", name)),
                );
                break;
            }
        }

        // Convert $HGPROF to profiling.type
        if let Ok(profiling_type) = env::var("HGPROF") {
            self.set("profiling", "type", Some(profiling_type), &"$HGPROF".into());
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
            if let Some(home_dir) = dirs::home_dir() {
                errors.append(&mut self.load_path(home_dir.join(".hgrc"), &opts));

                #[cfg(windows)]
                {
                    errors.append(&mut self.load_path(home_dir.join("mercurial.ini"), &opts));
                }
            }
            if let Some(config_dir) = dirs::config_dir() {
                errors.append(&mut self.load_path(config_dir.join("hg/hgrc"), &opts));
            }
        }

        errors
    }

    fn load_hgrc(&mut self, path: impl AsRef<Path>, source: &'static str) -> Vec<Error> {
        let opts = Options::new().source(source).process_hgplain();
        self.load_path(path, &opts)
    }

    fn get_opt<T: FromConfigValue>(&self, section: &str, name: &str) -> Result<Option<T>> {
        ConfigSet::get(self, section, name)
            .map(|bytes| T::try_from_str(&bytes))
            .transpose()
    }
}

impl FromConfigValue for bool {
    fn try_from_str(s: &str) -> Result<Self> {
        let value = s.to_lowercase();
        match value.as_ref() {
            "1" | "yes" | "true" | "on" | "always" => Ok(true),
            "0" | "no" | "false" | "off" | "never" => Ok(false),
            _ => Err(Error::Convert(format!("invalid bool: {}", value)).into()),
        }
    }
}

impl FromConfigValue for i8 {
    fn try_from_str(s: &str) -> Result<Self> {
        let value = s.parse()?;
        Ok(value)
    }
}

impl FromConfigValue for i16 {
    fn try_from_str(s: &str) -> Result<Self> {
        let value = s.parse()?;
        Ok(value)
    }
}

impl FromConfigValue for i32 {
    fn try_from_str(s: &str) -> Result<Self> {
        let value = s.parse()?;
        Ok(value)
    }
}

impl FromConfigValue for i64 {
    fn try_from_str(s: &str) -> Result<Self> {
        let value = s.parse()?;
        Ok(value)
    }
}

impl FromConfigValue for isize {
    fn try_from_str(s: &str) -> Result<Self> {
        let value = s.parse()?;
        Ok(value)
    }
}

impl FromConfigValue for u8 {
    fn try_from_str(s: &str) -> Result<Self> {
        let value = s.parse()?;
        Ok(value)
    }
}

impl FromConfigValue for u16 {
    fn try_from_str(s: &str) -> Result<Self> {
        let value = s.parse()?;
        Ok(value)
    }
}

impl FromConfigValue for u32 {
    fn try_from_str(s: &str) -> Result<Self> {
        let value = s.parse()?;
        Ok(value)
    }
}

impl FromConfigValue for u64 {
    fn try_from_str(s: &str) -> Result<Self> {
        let value = s.parse()?;
        Ok(value)
    }
}

impl FromConfigValue for usize {
    fn try_from_str(s: &str) -> Result<Self> {
        let value = s.parse()?;
        Ok(value)
    }
}

impl FromConfigValue for f32 {
    fn try_from_str(s: &str) -> Result<Self> {
        let value = s.parse()?;
        Ok(value)
    }
}

impl FromConfigValue for f64 {
    fn try_from_str(s: &str) -> Result<Self> {
        let value = s.parse()?;
        Ok(value)
    }
}

impl FromConfigValue for String {
    fn try_from_str(s: &str) -> Result<Self> {
        Ok(s.to_string())
    }
}

/// Byte count specified with a unit. For example: `1.5 MB`.
#[derive(Copy, Clone, Default)]
pub struct ByteCount(u64);

impl ByteCount {
    /// Get the value of bytes. For example, `1K` has a value of `1024`.
    pub fn value(self) -> u64 {
        self.0
    }
}

impl From<u64> for ByteCount {
    fn from(value: u64) -> ByteCount {
        ByteCount(value)
    }
}

impl FromConfigValue for ByteCount {
    fn try_from_str(s: &str) -> Result<Self> {
        // This implementation matches mercurial/util.py:sizetoint
        let sizeunits = [
            ("kb", 1u64 << 10),
            ("mb", 1 << 20),
            ("gb", 1 << 30),
            ("tb", 1 << 40),
            ("k", 1 << 10),
            ("m", 1 << 20),
            ("g", 1 << 30),
            ("t", 1 << 40),
            ("b", 1),
            ("", 1),
        ];

        let value = s.to_lowercase();
        for (suffix, unit) in sizeunits.iter() {
            if value.ends_with(suffix) {
                let number_str: &str = value[..value.len() - suffix.len()].trim();
                let number: f64 = number_str.parse()?;
                if number < 0.0 {
                    return Err(Error::Convert(format!(
                        "byte size '{:?}' cannot be negative",
                        value
                    ))
                    .into());
                }
                let unit = *unit as f64;
                return Ok(ByteCount((number * unit) as u64));
            }
        }

        Err(Error::Convert(format!("'{:?}' cannot be parsed as a byte size", value)).into())
    }
}

impl FromConfigValue for PathBuf {
    fn try_from_str(s: &str) -> Result<Self> {
        Ok(expand_path(s))
    }
}

impl<T: FromConfigValue> FromConfigValue for Vec<T> {
    fn try_from_str(s: &str) -> Result<Self> {
        let items = parse_list(s);
        items.into_iter().map(|s| T::try_from_str(&s)).collect()
    }
}

impl<T: FromConfigValue> FromConfigValue for Option<T> {
    fn try_from_str(s: &str) -> Result<Self> {
        T::try_from_str(s).map(Option::Some)
    }
}

/// Parse a configuration value as a list of comma/space separated strings.
/// It is ported from `mercurial.config.parselist`.
///
/// The function never complains about syntax and always returns some result.
///
/// Example:
///
/// ```
/// use configparser::hg::parse_list;
///
/// assert_eq!(
///     parse_list("this,is \"a small\" ,test"),
///     vec!["this".to_string(), "is".to_string(), "a small".to_string(), "test".to_string()]
/// );
/// ```
pub fn parse_list<B: AsRef<str>>(value: B) -> Vec<Text> {
    let mut value = value.as_ref();

    // ```python
    // if value is not None and isinstance(value, bytes):
    //     result = _configlist(value.lstrip(' ,\n'))
    // ```

    while [" ", ",", "\n"].iter().any(|b| value.starts_with(b)) {
        value = &value[1..]
    }

    parse_list_internal(value)
        .into_iter()
        .map(Text::from)
        .collect()
}

fn parse_list_internal(value: &str) -> Vec<String> {
    let mut value = value;

    // ```python
    // def _configlist(s):
    //     s = s.rstrip(' ,')
    //     if not s:
    //         return []
    //     parser, parts, offset = _parse_plain, [''], 0
    //     while parser:
    //         parser, parts, offset = parser(parts, s, offset)
    //     return parts
    // ```

    value = value.trim_end_matches(|c| " ,\n".contains(c));

    if value.is_empty() {
        return Vec::new();
    }

    #[derive(Copy, Clone)]
    enum State {
        Plain,
        Quote,
    };

    let mut offset = 0;
    let mut parts: Vec<String> = vec![String::new()];
    let mut state = State::Plain;
    let value: Vec<char> = value.chars().collect();

    loop {
        match state {
            // ```python
            // def _parse_plain(parts, s, offset):
            //     whitespace = False
            //     while offset < len(s) and (s[offset:offset + 1].isspace()
            //                                or s[offset:offset + 1] == ','):
            //         whitespace = True
            //         offset += 1
            //     if offset >= len(s):
            //         return None, parts, offset
            //     if whitespace:
            //         parts.append('')
            //     if s[offset:offset + 1] == '"' and not parts[-1]:
            //         return _parse_quote, parts, offset + 1
            //     elif s[offset:offset + 1] == '"' and parts[-1][-1:] == '\\':
            //         parts[-1] = parts[-1][:-1] + s[offset:offset + 1]
            //         return _parse_plain, parts, offset + 1
            //     parts[-1] += s[offset:offset + 1]
            //     return _parse_plain, parts, offset + 1
            // ```
            State::Plain => {
                let mut whitespace = false;
                while offset < value.len() && " \n\r\t,".contains(value[offset]) {
                    whitespace = true;
                    offset += 1;
                }
                if offset >= value.len() {
                    break;
                }
                if whitespace {
                    parts.push(Default::default());
                }
                if value[offset] == '"' {
                    let branch = {
                        match parts.last() {
                            None => 1,
                            Some(last) => {
                                if last.is_empty() {
                                    1
                                } else if last.ends_with('\\') {
                                    2
                                } else {
                                    3
                                }
                            }
                        }
                    }; // manual NLL, to drop reference on "parts".
                    if branch == 1 {
                        // last.is_empty()
                        state = State::Quote;
                        offset += 1;
                        continue;
                    } else if branch == 2 {
                        // last.ends_with(b"\\")
                        let last = parts.last_mut().unwrap();
                        last.pop();
                        last.push(value[offset]);
                        offset += 1;
                        continue;
                    }
                }
                let last = parts.last_mut().unwrap();
                last.push(value[offset]);
                offset += 1;
            }

            // ```python
            // def _parse_quote(parts, s, offset):
            //     if offset < len(s) and s[offset:offset + 1] == '"': # ""
            //         parts.append('')
            //         offset += 1
            //         while offset < len(s) and (s[offset:offset + 1].isspace() or
            //                 s[offset:offset + 1] == ','):
            //             offset += 1
            //         return _parse_plain, parts, offset
            //     while offset < len(s) and s[offset:offset + 1] != '"':
            //         if (s[offset:offset + 1] == '\\' and offset + 1 < len(s)
            //                 and s[offset + 1:offset + 2] == '"'):
            //             offset += 1
            //             parts[-1] += '"'
            //         else:
            //             parts[-1] += s[offset:offset + 1]
            //         offset += 1
            //     if offset >= len(s):
            //         real_parts = _configlist(parts[-1])
            //         if not real_parts:
            //             parts[-1] = '"'
            //         else:
            //             real_parts[0] = '"' + real_parts[0]
            //             parts = parts[:-1]
            //             parts.extend(real_parts)
            //         return None, parts, offset
            //     offset += 1
            //     while offset < len(s) and s[offset:offset + 1] in [' ', ',']:
            //         offset += 1
            //     if offset < len(s):
            //         if offset + 1 == len(s) and s[offset:offset + 1] == '"':
            //             parts[-1] += '"'
            //             offset += 1
            //         else:
            //             parts.append('')
            //     else:
            //         return None, parts, offset
            //     return _parse_plain, parts, offset
            // ```
            State::Quote => {
                if offset < value.len() && value[offset] == '"' {
                    parts.push(Default::default());
                    offset += 1;
                    while offset < value.len() && " \n\r\t,".contains(value[offset]) {
                        offset += 1;
                    }
                    state = State::Plain;
                    continue;
                }
                while offset < value.len() && value[offset] != '"' {
                    if value[offset] == '\\' && offset + 1 < value.len() && value[offset + 1] == '"'
                    {
                        offset += 1;
                        parts.last_mut().unwrap().push('"');
                    } else {
                        parts.last_mut().unwrap().push(value[offset]);
                    }
                    offset += 1;
                }
                if offset >= value.len() {
                    let mut real_parts: Vec<String> = parse_list_internal(parts.last().unwrap());
                    if real_parts.is_empty() {
                        parts.pop();
                        parts.push("\"".to_string());
                    } else {
                        real_parts[0].insert(0, '"');
                        parts.pop();
                        parts.append(&mut real_parts);
                    }
                    break;
                }
                offset += 1;
                while offset < value.len() && " ,".contains(value[offset]) {
                    offset += 1;
                }
                if offset < value.len() {
                    if offset + 1 == value.len() && value[offset] == '"' {
                        parts.last_mut().unwrap().push('"');
                        offset += 1;
                    } else {
                        parts.push(Default::default());
                    }
                } else {
                    break;
                }
                state = State::Plain;
            }
        }
    }

    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    use tempdir::TempDir;

    use crate::config::tests::write_file;

    use lazy_static::lazy_static;
    use parking_lot::Mutex;

    lazy_static! {
        /// Lock for the environment.  This should be acquired by tests that rely on particular
        /// environment variable values that might be overwritten by other tests.
        static ref ENV_LOCK: Mutex<()> = Mutex::new(());
    }

    #[test]
    fn test_basic_hgplain() {
        let _guard = ENV_LOCK.lock();
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
        let _guard = ENV_LOCK.lock();
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

        cfg.load_system();
        assert!(cfg.sections().is_empty());

        cfg.load_user();
        assert_eq!(cfg.get("x", "a"), Some("1".into()));
        assert_eq!(cfg.get("y", "b"), Some("2".into()));
    }

    #[test]
    fn test_load_hgrc() {
        let dir = TempDir::new("test_hgrcpath").unwrap();
        let path = dir.path().join("1.rc");

        write_file(path.clone(), "[x]\na=1\n[alias]\nb=c\n");

        let _guard = ENV_LOCK.lock();
        env::set_var(HGPLAIN, "1");
        env::remove_var(HGPLAINEXCEPT);

        let mut cfg = ConfigSet::new();
        cfg.load_hgrc(&path, "hgrc");

        assert!(cfg.keys("alias").is_empty());
        assert!(cfg.get("alias", "b").is_none());
        assert_eq!(cfg.get("x", "a").unwrap(), "1");

        env::remove_var(HGPLAIN);
        cfg.load_hgrc(&path, "hgrc");

        assert_eq!(cfg.get("alias", "b").unwrap(), "c");
    }

    #[test]
    fn test_section_filter() {
        let opts = Options::new().filter_sections(vec!["x", "y"]);
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

        assert_eq!(cfg.sections(), vec![Text::from("x"), Text::from("y")]);
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

    #[test]
    fn test_parse_list() {
        fn b<B: AsRef<str>>(bytes: B) -> Text {
            Text::copy_from_slice(bytes.as_ref())
        }

        // From test-ui-config.py
        assert_eq!(parse_list("foo"), vec![b("foo")]);
        assert_eq!(
            parse_list("foo bar baz"),
            vec![b("foo"), b("bar"), b("baz")]
        );
        assert_eq!(parse_list("alice, bob"), vec![b("alice"), b("bob")]);
        assert_eq!(
            parse_list("foo bar baz alice, bob"),
            vec![b("foo"), b("bar"), b("baz"), b("alice"), b("bob")]
        );
        assert_eq!(
            parse_list("abc d\"ef\"g \"hij def\""),
            vec![b("abc"), b("d\"ef\"g"), b("hij def")]
        );
        assert_eq!(
            parse_list("\"hello world\", \"how are you?\""),
            vec![b("hello world"), b("how are you?")]
        );
        assert_eq!(
            parse_list("Do\"Not\"Separate"),
            vec![b("Do\"Not\"Separate")]
        );
        assert_eq!(parse_list("\"Do\"Separate"), vec![b("Do"), b("Separate")]);
        assert_eq!(
            parse_list("\"Do\\\"NotSeparate\""),
            vec![b("Do\"NotSeparate")]
        );
        assert_eq!(
            parse_list("string \"with extraneous\" quotation mark\""),
            vec![
                b("string"),
                b("with extraneous"),
                b("quotation"),
                b("mark\""),
            ]
        );
        assert_eq!(parse_list("x, y"), vec![b("x"), b("y")]);
        assert_eq!(parse_list("\"x\", \"y\""), vec![b("x"), b("y")]);
        assert_eq!(
            parse_list("\"\"\" key = \"x\", \"y\" \"\"\""),
            vec![b(""), b(" key = "), b("x\""), b("y"), b(""), b("\"")]
        );
        assert_eq!(parse_list(",,,,     "), Vec::<Text>::new());
        assert_eq!(
            parse_list("\" just with starting quotation"),
            vec![b("\""), b("just"), b("with"), b("starting"), b("quotation")]
        );
        assert_eq!(
            parse_list("\"longer quotation\" with \"no ending quotation"),
            vec![
                b("longer quotation"),
                b("with"),
                b("\"no"),
                b("ending"),
                b("quotation"),
            ]
        );
        assert_eq!(
            parse_list("this is \\\" \"not a quotation mark\""),
            vec![b("this"), b("is"), b("\""), b("not a quotation mark")]
        );
        assert_eq!(parse_list("\n \n\nding\ndong"), vec![b("ding"), b("dong")]);

        // Other manually written cases
        assert_eq!(parse_list("a,b,,c"), vec![b("a"), b("b"), b("c")]);
        assert_eq!(parse_list("a b  c"), vec![b("a"), b("b"), b("c")]);
        assert_eq!(
            parse_list(" , a , , b,  , c , "),
            vec![b("a"), b("b"), b("c")]
        );
        assert_eq!(parse_list("a,\"b,c\" d"), vec![b("a"), b("b,c"), b("d")]);
        assert_eq!(parse_list("a,\",c"), vec![b("a"), b("\""), b("c")]);
        assert_eq!(parse_list("a,\" c\" \""), vec![b("a"), b(" c\"")]);
        assert_eq!(
            parse_list("a,\" c\" \" d"),
            vec![b("a"), b(" c"), b("\""), b("d")]
        );
    }

    #[test]
    fn test_get_or() {
        let mut cfg = ConfigSet::new();
        cfg.parse(
            "[foo]\n\
             bool1 = yes\n\
             bool2 = unknown\n\
             bools = 1, TRUE, On, aLwAys, 0, false, oFF, never\n\
             int1 = -33\n\
             list1 = x y z\n\
             list3 = 2, 3, 1\n\
             byte1 = 1.5 KB\n\
             byte2 = 500\n\
             byte3 = 0.125M\n\
             float = 1.42\n\
             ",
            &"test".into(),
        );

        assert_eq!(cfg.get_or("foo", "bar", || 3).unwrap(), 3);
        assert_eq!(cfg.get_or("foo", "bool1", || false).unwrap(), true);
        assert_eq!(
            format!("{}", cfg.get_or("foo", "bool2", || true).unwrap_err()),
            "invalid bool: unknown"
        );
        assert_eq!(cfg.get_or("foo", "int1", || 42).unwrap(), -33);
        assert_eq!(
            cfg.get_or("foo", "list1", || vec!["x".to_string()])
                .unwrap(),
            vec!["x", "y", "z"]
        );
        assert_eq!(
            cfg.get_or("foo", "list3", || vec![0]).unwrap(),
            vec![2, 3, 1]
        );

        assert_eq!(cfg.get_or_default::<bool>("foo", "bool1").unwrap(), true);
        assert_eq!(
            cfg.get_or_default::<Vec<bool>>("foo", "bools").unwrap(),
            vec![true, true, true, true, false, false, false, false]
        );

        assert_eq!(
            cfg.get_or_default::<ByteCount>("foo", "byte1")
                .unwrap()
                .value(),
            1536
        );
        assert_eq!(
            cfg.get_or_default::<ByteCount>("foo", "byte2")
                .unwrap()
                .value(),
            500
        );
        assert_eq!(
            cfg.get_or_default::<ByteCount>("foo", "byte3")
                .unwrap()
                .value(),
            131072
        );
        assert_eq!(
            cfg.get_or("foo", "missing", || ByteCount::from(3))
                .unwrap()
                .value(),
            3
        );
        assert_eq!(cfg.get_or("foo", "float", || 42f32).unwrap(), 1.42f32);
    }
}

const MERGE_TOOLS_CONFIG: &str = r#"# Some default global settings for common merge tools

[merge-tools]
kdiff3.args=--auto --L1 base --L2 local --L3 other $base $local $other -o $output
kdiff3.regkey=Software\KDiff3
kdiff3.regkeyalt=Software\Wow6432Node\KDiff3
kdiff3.regappend=\kdiff3.exe
kdiff3.fixeol=True
kdiff3.gui=True
kdiff3.diffargs=--L1 $plabel1 --L2 $clabel $parent $child

gvimdiff.args=--nofork -d -g -O $local $other $base
gvimdiff.regkey=Software\Vim\GVim
gvimdiff.regkeyalt=Software\Wow6432Node\Vim\GVim
gvimdiff.regname=path
gvimdiff.priority=-9
gvimdiff.diffargs=--nofork -d -g -O $parent $child

vimdiff.args=$local $other $base -c 'redraw | echomsg "hg merge conflict, type \":cq\" to abort vimdiff"'
vimdiff.check=changed
vimdiff.priority=-10

merge.check=conflicts
merge.priority=-100

gpyfm.gui=True

meld.gui=True
meld.args=--label='local' $local --label='merged' $base --label='other' $other -o $output
meld.check=changed
meld.diffargs=-a --label=$plabel1 $parent --label=$clabel $child

tkdiff.args=$local $other -a $base -o $output
tkdiff.gui=True
tkdiff.priority=-8
tkdiff.diffargs=-L $plabel1 $parent -L $clabel $child

xxdiff.args=--show-merged-pane --exit-with-merge-status --title1 local --title2 base --title3 other --merged-filename $output --merge $local $base $other
xxdiff.gui=True
xxdiff.priority=-8
xxdiff.diffargs=--title1 $plabel1 $parent --title2 $clabel $child

diffmerge.regkey=Software\SourceGear\SourceGear DiffMerge\
diffmerge.regkeyalt=Software\Wow6432Node\SourceGear\SourceGear DiffMerge\
diffmerge.regname=Location
diffmerge.priority=-7
diffmerge.args=-nosplash -merge -title1=local -title2=merged -title3=other $local $base $other -result=$output
diffmerge.check=changed
diffmerge.gui=True
diffmerge.diffargs=--nosplash --title1=$plabel1 --title2=$clabel $parent $child

p4merge.args=$base $local $other $output
p4merge.regkey=Software\Perforce\Environment
p4merge.regkeyalt=Software\Wow6432Node\Perforce\Environment
p4merge.regname=P4INSTROOT
p4merge.regappend=\p4merge.exe
p4merge.gui=True
p4merge.priority=-8
p4merge.diffargs=$parent $child

p4mergeosx.executable = /Applications/p4merge.app/Contents/MacOS/p4merge
p4mergeosx.args = $base $local $other $output
p4mergeosx.gui = True
p4mergeosx.priority=-8
p4mergeosx.diffargs=$parent $child

tortoisemerge.args=/base:$base /mine:$local /theirs:$other /merged:$output
tortoisemerge.regkey=Software\TortoiseSVN
tortoisemerge.regkeyalt=Software\Wow6432Node\TortoiseSVN
tortoisemerge.check=changed
tortoisemerge.gui=True
tortoisemerge.priority=-8
tortoisemerge.diffargs=/base:$parent /mine:$child /basename:$plabel1 /minename:$clabel

ecmerge.args=$base $local $other --mode=merge3 --title0=base --title1=local --title2=other --to=$output
ecmerge.regkey=Software\Elli\xc3\xa9 Computing\Merge
ecmerge.regkeyalt=Software\Wow6432Node\Elli\xc3\xa9 Computing\Merge
ecmerge.gui=True
ecmerge.diffargs=$parent $child --mode=diff2 --title1=$plabel1 --title2=$clabel

# editmerge is a small script shipped in contrib.
# It needs this config otherwise it behaves the same as internal:local
editmerge.args=$output
editmerge.check=changed
editmerge.premerge=keep

filemerge.executable=/Developer/Applications/Utilities/FileMerge.app/Contents/MacOS/FileMerge
filemerge.args=-left $other -right $local -ancestor $base -merge $output
filemerge.gui=True

filemergexcode.executable=/Applications/Xcode.app/Contents/Applications/FileMerge.app/Contents/MacOS/FileMerge
filemergexcode.args=-left $other -right $local -ancestor $base -merge $output
filemergexcode.gui=True

; Windows version of Beyond Compare
beyondcompare3.args=$local $other $base $output /ro /lefttitle=local /centertitle=base /righttitle=other /automerge /reviewconflicts /solo
beyondcompare3.regkey=Software\Scooter Software\Beyond Compare 3
beyondcompare3.regname=ExePath
beyondcompare3.gui=True
beyondcompare3.priority=-2
beyondcompare3.diffargs=/lro /lefttitle=$plabel1 /righttitle=$clabel /solo /expandall $parent $child

; Linux version of Beyond Compare
bcompare.args=$local $other $base -mergeoutput=$output -ro -lefttitle=parent1 -centertitle=base -righttitle=parent2 -outputtitle=merged -automerge -reviewconflicts -solo
bcompare.gui=True
bcompare.priority=-1
bcompare.diffargs=-lro -lefttitle=$plabel1 -righttitle=$clabel -solo -expandall $parent $child

; OS X version of Beyond Compare
bcomposx.executable = /Applications/Beyond Compare.app/Contents/MacOS/bcomp
bcomposx.args=$local $other $base -mergeoutput=$output -ro -lefttitle=parent1 -centertitle=base -righttitle=parent2 -outputtitle=merged -automerge -reviewconflicts -solo
bcomposx.gui=True
bcomposx.priority=-1
bcomposx.diffargs=-lro -lefttitle=$plabel1 -righttitle=$clabel -solo -expandall $parent $child

winmerge.args=/e /x /wl /ub /dl other /dr local $other $local $output
winmerge.regkey=Software\Thingamahoochie\WinMerge
winmerge.regkeyalt=Software\Wow6432Node\Thingamahoochie\WinMerge\
winmerge.regname=Executable
winmerge.check=changed
winmerge.gui=True
winmerge.priority=-10
winmerge.diffargs=/r /e /x /ub /wl /dl $plabel1 /dr $clabel $parent $child

araxis.regkey=SOFTWARE\Classes\TypeLib\{46799e0a-7bd1-4330-911c-9660bb964ea2}\7.0\HELPDIR
araxis.regappend=\ConsoleCompare.exe
araxis.priority=-2
araxis.args=/3 /a2 /wait /merge /title1:"Other" /title2:"Base" /title3:"Local :"$local $other $base $local $output
araxis.checkconflict=True
araxis.binary=True
araxis.gui=True
araxis.diffargs=/2 /wait /title1:$plabel1 /title2:$clabel $parent $child

diffuse.priority=-3
diffuse.args=$local $base $other
diffuse.gui=True
diffuse.diffargs=$parent $child

UltraCompare.regkey=Software\Microsoft\Windows\CurrentVersion\App Paths\UC.exe
UltraCompare.regkeyalt=Software\Wow6432Node\Microsoft\Windows\CurrentVersion\App Paths\UC.exe
UltraCompare.args = $base $local $other -title1 base -title3 other
UltraCompare.priority = -2
UltraCompare.gui = True
UltraCompare.binary = True
UltraCompare.check = conflicts,changed
UltraCompare.diffargs=$child $parent -title1 $clabel -title2 $plabel1
"#;
