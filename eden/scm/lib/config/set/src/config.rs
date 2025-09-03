/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::borrow::Cow;
use std::collections::HashSet;
use std::fmt;
use std::fmt::Display;
use std::fmt::Formatter;
use std::fs;
use std::hash::Hash;
use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;
use std::str;
use std::sync::Arc;

use configmodel::Config;
pub use configmodel::ValueLocation;
pub use configmodel::ValueSource;
use configmodel::config::ContentHash;
use hgrc_parser::Instruction;
use hgrc_parser::parse;
use indexmap::IndexMap;
use indexmap::IndexSet;
use minibytes::Text;
use util::path::expand_path;

use crate::error::Error;

/// Collection of config sections loaded from various sources.
#[derive(Clone, Default)]
pub struct ConfigSet {
    name: Text,

    // Max priority values that should always be remembered/maintained.
    // These include --config CLI values and runtime config overrides.
    // These take priority over `sections` and `secondary`.
    pinned: IndexMap<Text, Section>,

    // Regular priority values. This is where `load_file()` and `set()` go by default.
    sections: IndexMap<Text, Section>,

    // Secondary, immutable config to try out if `sections` and `pinned` do not contain
    // the requested config.
    secondary: Option<Arc<dyn Config>>,

    // Canonicalized files that were loaded, including files with errors.
    // Value is a hash of file contents, if file was readable.
    files: Vec<(PathBuf, Option<ContentHash>)>,
}

/// Internal representation of a config section.
#[derive(Clone, Default, Debug)]
struct Section {
    items: IndexMap<Text, Vec<ValueSource>>,
}

/// Options that affects config setting functions like `load_path`, `parse`,
/// and `set`.
#[derive(Clone, Default)]
pub struct Options {
    source: Text,
    filters: Vec<Rc<Box<dyn Fn(Text, Text, Option<Text>) -> Option<(Text, Text, Option<Text>)>>>>,
    pin: Option<bool>,

    /// Minimize cases where we regenerate the dynamic config synchronously.
    /// This is useful for programs that embed us (like EdenFS) to avoid dynamic config
    /// flapping due to version string mismatch.
    pub minimize_dynamic_gen: bool,

    /// Don't fetch any remote configs. Error out if local caches are empty.
    pub local_only: bool,
}

impl Config for ConfigSet {
    /// Get config names under a section, sorted by insertion order.
    ///
    /// keys("foo") returns keys in section "foo".
    fn keys(&self, section: &str) -> Vec<Text> {
        let pinned_keys: Cow<[Text]> = Cow::Owned(
            self.pinned
                .get(section)
                .map(|section| section.items.keys().cloned().collect())
                .unwrap_or_default(),
        );

        let main_keys: Cow<[Text]> = Cow::Owned(
            self.sections
                .get(section)
                .map(|section| section.items.keys().cloned().collect())
                .unwrap_or_default(),
        );

        let self_keys = merge_cow_list(pinned_keys, main_keys);

        if let Some(secondary) = &self.secondary {
            let secondary_keys = secondary.keys(section);
            let result = merge_cow_list(Cow::Owned(secondary_keys), self_keys);
            result.into_owned()
        } else {
            self_keys.into_owned()
        }
    }

    /// Get config value for a given config.
    /// Return `None` if the config item does not exist.
    /// Return `Some(None)` if the config is is unset.
    fn get_considering_unset(&self, section: &str, name: &str) -> Option<Option<Text>> {
        let get_self_value = |sections: &IndexMap<Text, Section>| -> Option<Option<Text>> {
            let section = sections.get(section)?;
            let value_sources: &Vec<ValueSource> = section.items.get(name)?;
            let value = value_sources.last()?.value.clone();
            Some(value)
        };

        let self_value = get_self_value(&self.pinned).or_else(|| get_self_value(&self.sections));

        if let (None, Some(secondary)) = (&self_value, &self.secondary) {
            return secondary.get_considering_unset(section, name);
        }
        self_value
    }

    /// Get config sections.
    fn sections(&self) -> Cow<'_, [Text]> {
        let pinned: Cow<[Text]> = Cow::Owned(self.pinned.keys().cloned().collect());
        let main: Cow<[Text]> = Cow::Owned(self.sections.keys().cloned().collect());
        let self_sections = merge_cow_list(pinned, main);
        if let Some(secondary) = &self.secondary {
            let secondary_sections = secondary.sections();
            merge_cow_list(secondary_sections, self_sections)
        } else {
            self_sections
        }
    }

    /// Get detailed sources of a given config, including overrides, and source information.
    /// The last item in the returned vector is the latest value that is considered effective.
    ///
    /// Return an empty vector if the config does not exist.
    fn get_sources(&self, section: &str, name: &str) -> Cow<'_, [ValueSource]> {
        let pinned_sources = self
            .pinned
            .get(section)
            .and_then(|section| section.items.get(name));

        let main_sources = self
            .sections
            .get(section)
            .and_then(|section| section.items.get(name));

        let self_sources: Cow<[ValueSource]> = match (pinned_sources, main_sources) {
            (None, None) => Cow::Owned(Vec::new()),
            (Some(pinned), None) => Cow::Borrowed(pinned),
            (None, Some(main)) => Cow::Borrowed(main),
            (Some(pinned), Some(main)) => Cow::Owned(main.iter().chain(pinned).cloned().collect()),
        };

        if let Some(secondary) = &self.secondary {
            let secondary_sources = secondary.get_sources(section, name);
            if secondary_sources.is_empty() {
                self_sources
            } else if self_sources.is_empty() {
                secondary_sources
            } else {
                let sources: Vec<ValueSource> = secondary_sources
                    .iter()
                    .cloned()
                    .chain(self_sources.into_owned())
                    .collect();
                Cow::Owned(sources)
            }
        } else {
            self_sources
        }
    }

    /// Get on-disk files loaded for this `Config`.
    fn files(&self) -> Cow<'_, [(PathBuf, Option<ContentHash>)]> {
        let self_files: Cow<[(PathBuf, Option<ContentHash>)]> = Cow::Borrowed(&self.files);
        if let Some(secondary) = &self.secondary {
            let secondary_files = secondary.files();
            // file load order: secondary first
            merge_cow_list(secondary_files, self_files)
        } else {
            self_files
        }
    }

    fn layer_name(&self) -> Text {
        if self.name.is_empty() {
            Text::from_static("ConfigSet")
        } else {
            self.name.clone()
        }
    }

    fn layers(&self) -> Vec<Arc<dyn Config>> {
        if let Some(secondary) = &self.secondary {
            let mut layers = secondary.layers();
            if !self.sections.is_empty() {
                // PERF: This clone can be slow.
                let mut primary = self.clone().named("primary");
                primary.secondary = None;
                layers.push(Arc::new(primary))
            }
            layers
        } else {
            Vec::new()
        }
    }

    fn pinned(&self) -> Vec<(Text, Text, Vec<ValueSource>)> {
        self.pinned
            .iter()
            .flat_map(|(sname, svalues)| {
                svalues
                    .items
                    .iter()
                    .map(|(kname, values)| (sname.clone(), kname.clone(), values.to_vec()))
            })
            .collect()
    }
}

/// Merge two lists. Preserve order (a is before b). Remove duplicated items.
/// Assumes `a` and `b` do not have duplicated items respectively.
fn merge_cow_list<'a, T: Clone + Hash + Eq>(a: Cow<'a, [T]>, b: Cow<'a, [T]>) -> Cow<'a, [T]> {
    if a.is_empty() {
        b
    } else if b.is_empty() {
        a
    } else {
        let result: IndexSet<T> = a.iter().cloned().chain(b.iter().cloned()).collect();
        let result: Vec<T> = result.into_iter().collect();
        Cow::Owned(result)
    }
}

impl Display for ConfigSet {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        for section in self.sections().iter() {
            writeln!(f, "[{}]", section.as_ref())?;

            for key in self.keys(section).iter() {
                let value = self.get_considering_unset(section, key);
                #[cfg(test)]
                {
                    let values = self.get_sources(section, key);
                    assert_eq!(values.last().map(|v| v.value().clone()), value);
                }
                if let Some(value) = value {
                    if let Some(value) = value {
                        // When a newline delimited list is loaded, the whitespace around each
                        // entry is trimmed. In order for the serialized config to be parsable, we
                        // need some indentation after each newline. Since this whitespace will be
                        // stripped on load, it shouldn't hurt anything.
                        writeln!(f, "{}={}", key, value.replace('\n', "\n "))?;
                    } else {
                        // None indicates the value was unset.
                        writeln!(f, "%unset {}", key)?;
                    }
                }
            }

            writeln!(f)?;
        }
        Ok(())
    }
}

impl ConfigSet {
    /// Return an empty `ConfigSet`.
    pub fn new() -> Self {
        Default::default()
    }

    /// Create a (mutable) ConfigSet wrapping `config`.
    /// This allows you to overlay new configs on top of `config`.
    pub fn wrap(config: Arc<dyn Config>) -> Self {
        let mut wrapped = Self {
            secondary: Some(config.clone()),
            ..Default::default()
        };

        for (sname, kname, values) in config.pinned() {
            wrapped
                .pinned
                .entry(sname)
                .or_default()
                .items
                .insert(kname, values);
        }

        wrapped
    }

    /// Attach a secondary config as fallback for items missing from the
    /// main config.
    ///
    /// The secondary config is immutable, is cheaper to clone, and provides
    /// the layers information.
    ///
    /// If a secondary config was already attached, it will be replaced by this
    /// call.
    pub fn secondary(&mut self, secondary: Arc<dyn Config>) -> &mut Self {
        self.secondary = Some(secondary);
        self
    }

    /// Update the name of the `ConfigSet`.
    pub fn named(mut self, name: &str) -> Self {
        self.name = Text::copy_from_slice(name);
        self
    }

    /// Load config files at given path. The path is a file.
    ///
    /// If `path` is a directory, it is ignored.
    /// If `path` is a file, it will be loaded directly.
    ///
    /// A config file can use `%include` to load other paths (directories or files). They will
    /// be loaded recursively. Includes take effect in place, instead of deferred. For example,
    /// with the following two files:
    ///
    /// ```plain,ignore
    /// # This is 1.rc
    /// [section]
    /// x = 1
    /// %include 2.rc
    /// y = 2
    ///
    /// # This is 2.rc
    /// [section]
    /// x = 3
    /// y = 4
    /// ```
    ///
    /// After loading `1.rc`. `x` is set to 3 and `y` is set to 2.
    ///
    /// Loading a file that is already parsed or being parsed by this `load_path` call is ignored,
    /// to avoid infinite loop. A separate `load_path` call would not ignore files loaded by
    /// other `load_path` calls.
    ///
    /// Return a list of errors. An error parsing a file will stop that file from loading, without
    /// affecting other files.
    pub fn load_path<P: AsRef<Path>>(&mut self, path: P, opts: &Options) -> Vec<Error> {
        let mut visited = HashSet::new();
        let mut errors = Vec::new();
        self.load_file(path.as_ref(), opts, &mut visited, &mut errors);
        errors
    }

    /// Load content of an unnamed config file. The `ValueLocation`s of loaded config items will
    /// have an empty `path`.
    ///
    /// Return a list of errors.
    pub fn parse<B: Into<Text>>(&mut self, content: B, opts: &Options) -> Vec<Error> {
        let mut visited = HashSet::new();
        let mut errors = Vec::new();
        let buf = content.into();
        self.load_file_content(Path::new(""), buf, opts, &mut visited, &mut errors);
        errors
    }

    /// Set a config item directly. `section`, `name` locates the config. `value` is the new value.
    /// `source` is some annotation about who set it, ex. "reporc", "userrc", "--config", etc.
    /// Value is set as a "pinned" config which will "stick" if the config is re-loaded.
    pub fn set(
        &mut self,
        section: impl AsRef<str>,
        name: impl AsRef<str>,
        value: Option<impl AsRef<str>>,
        opts: &Options,
    ) {
        let section = Text::copy_from_slice(section.as_ref());
        let name = Text::copy_from_slice(name.as_ref());
        let value = value.map(|v| Text::copy_from_slice(v.as_ref()));
        self.set_internal(section, name, value, None, opts)
    }

    fn set_internal(
        &mut self,
        section: Text,
        name: Text,
        value: Option<Text>,
        location: Option<ValueLocation>,
        opts: &Options,
    ) {
        if let Some((section, name, value)) = opts.filter(section, name, value) {
            let dest = if opts.pin.unwrap_or(true) {
                &mut self.pinned
            } else {
                &mut self.sections
            };
            dest.entry(section)
                .or_insert_with(Default::default)
                .items
                .entry(name)
                .or_insert_with(|| Vec::with_capacity(1))
                .push(ValueSource {
                    value,
                    location,
                    source: opts.source.clone(),
                })
        }
    }

    fn load_file(
        &mut self,
        path: &Path,
        opts: &Options,
        visited: &mut HashSet<PathBuf>,
        errors: &mut Vec<Error>,
    ) {
        match path.canonicalize() {
            Ok(path) => {
                let path = &path;
                debug_assert!(path.is_absolute());

                if !visited.insert(path.to_path_buf()) {
                    // skip - visited before
                    return;
                }

                match fs::read_to_string(path) {
                    Ok(mut text) => {
                        self.files.push((
                            path.to_path_buf(),
                            Some(ContentHash::from_contents(text.as_bytes())),
                        ));

                        text.push('\n');
                        let text = Text::from(text);
                        self.load_file_content(path, text, opts, visited, errors);
                    }
                    Err(error) => {
                        self.files.push((path.to_path_buf(), None));

                        errors.push(Error::Io(path.to_path_buf(), error))
                    }
                }
            }
            Err(err) => {
                // On Windows, a UNC path `\\?\C:\foo\.\x` will fail to canonicalize
                // because it contains `.`. That path can be constructed by using
                // `PathBuf::join` to concatenate a UNC path `\\?\C:\foo` with
                // a "normal" path `.\x`.
                // Try to fix it automatically by stripping the UNC prefix and retry
                // `canonicalize`. `C:\foo\.\x` would be canonicalized without errors.
                if cfg!(windows) {
                    if let Some(without_unc) = path.to_str().and_then(|p| p.strip_prefix(r"\\?\")) {
                        self.load_file(without_unc.as_ref(), opts, visited, errors);
                        return;
                    }
                }

                tracing::debug!(?err, ?path, "not loading config file");

                // If it is absolute, record it in `files` anyway. This is important to
                // record that we've loaded the repo's config file even if the config file
                // doesn't exist.
                if path.is_absolute() {
                    self.files.push((path.to_path_buf(), None));
                }
            }
        }

        // If `path.canonicalize` reports an error. It's usually the path cannot
        // be resolved (ex. does not exist). It is considered normal and is not
        // reported in `errors`.
    }

    fn load_file_content(
        &mut self,
        path: &Path,
        buf: Text,
        opts: &Options,
        visited: &mut HashSet<PathBuf>,
        errors: &mut Vec<Error>,
    ) {
        tracing::debug!(
            "load {} from path '{}' ({} bytes)",
            path.display(),
            opts.source.as_ref(),
            buf.len()
        );

        let shared_path = Arc::new(path.to_path_buf()); // use Arc to do shallow copy
        let skip_include = path.parent().is_none(); // skip handling %include if path is empty

        let insts = match parse(&buf) {
            Ok(insts) => insts,
            Err(error) => {
                return errors.push(Error::ParseFile(path.to_path_buf(), format!("{}", error)));
            }
        };

        for inst in insts {
            match inst {
                Instruction::SetConfig {
                    section,
                    name,
                    value,
                    span,
                } => {
                    let section = buf.slice_to_bytes(section);
                    let name = buf.slice_to_bytes(name);
                    let value = Some(buf.slice_to_bytes(&value));
                    let location = ValueLocation {
                        path: shared_path.clone(),
                        content: buf.clone(),
                        location: span,
                    };
                    self.set_internal(section, name, value, location.into(), opts);
                }
                Instruction::UnsetConfig {
                    section,
                    name,
                    span,
                } => {
                    let section = buf.slice_to_bytes(section);
                    let name = buf.slice_to_bytes(name);
                    let location = ValueLocation {
                        path: shared_path.clone(),
                        content: buf.clone(),
                        location: span,
                    };
                    self.set_internal(section.clone(), name, None, location.into(), opts);
                }
                Instruction::Include {
                    path: include_path,
                    span: _,
                } => {
                    if !skip_include {
                        if let Some(content) = crate::builtin::get(include_path) {
                            if !content.is_empty() {
                                let text = Text::from(content);
                                let path = Path::new(include_path);
                                self.load_file_content(path, text, opts, visited, errors);
                            }
                        } else {
                            let full_include_path =
                                path.parent().unwrap().join(expand_path(include_path));
                            self.load_file(&full_include_path, opts, visited, errors);
                        }
                    }
                }
            }
        }
    }

    pub fn clear_unpinned(&mut self) {
        self.sections.clear();
        self.secondary = None;

        // Not technically correct since "pinned" configs could have
        // been loaded from files, but probably doesn't matter either
        // way.
        self.files.clear();
    }
}

impl Options {
    /// Create a default `Options`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a filter. A filter can decide to ignore a config item, or change its section,
    /// config name, or even value. The filter function takes a tuple of `(section, name, value)`
    /// and outputs `None` to prevent inserting that value, or `Some((section, name, value))` to
    /// insert it with optionally different name or values.
    ///
    /// Filters inserted first will be executed first.
    pub fn append_filter(
        mut self,
        filter: Box<dyn Fn(Text, Text, Option<Text>) -> Option<(Text, Text, Option<Text>)>>,
    ) -> Self {
        self.filters.push(Rc::new(filter));
        self
    }

    /// Set `source` information. It is about who initialized the config loading.  For example,
    /// "user_hgrc" indicates it is from the user config file, "--config" indicates it is from the
    /// global "--config" command line flag, "env" indicates it is translated from an environment
    /// variable (ex.  "PAGER"), etc.
    pub fn source<B: Into<Text>>(mut self, source: B) -> Self {
        self.source = source.into();
        self
    }

    /// Pass `(section, name, value)` through chain of filters, yielding mutated
    /// result or `None`, if any filter returned `None`.
    pub fn filter(
        &self,
        section: Text,
        name: Text,
        value: Option<Text>,
    ) -> Option<(Text, Text, Option<Text>)> {
        self.filters
            .iter()
            .try_fold((section, name, value), move |(s, n, v), func| func(s, n, v))
    }

    /// Mark config insertions as "pinned". This places them in a higher priority area
    /// separate from regular configs, making them easier to maintain.
    pub fn pin(mut self, pin: bool) -> Self {
        self.pin = Some(pin);
        self
    }
}

/// Convert a "source" string to an `Options`.
impl<S: Into<Text>> From<S> for Options {
    fn from(source: S) -> Options {
        Options::new().source(source.into())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::io::Write;

    use configmodel::ConfigExt;
    use tempfile::TempDir;

    use super::*;
    use crate::convert::ByteCount;

    #[test]
    fn test_empty() {
        let cfg = ConfigSet::new();
        assert!(cfg.sections().is_empty());
        assert!(cfg.keys("foo").is_empty());
        assert!(cfg.get("foo", "bar").is_none());
        assert!(cfg.get_sources("foo", "bar").is_empty());
    }

    #[test]
    fn test_set() {
        let mut cfg = ConfigSet::new();
        cfg.set("y", "b", Some("1"), &"set1".into());
        cfg.set("y", "b", Some("2"), &"set2".into());
        cfg.set("y", "a", Some("3"), &"set3".into());
        cfg.set("z", "p", Some("4"), &"set4".into());
        cfg.set("z", "p", None::<Text>, &"set5".into());
        assert_eq!(cfg.sections(), vec![Text::from("y"), Text::from("z")]);
        assert_eq!(cfg.keys("y"), vec![Text::from("b"), Text::from("a")]);
        assert_eq!(cfg.get("y", "b"), Some(Text::from("2")));
        assert_eq!(cfg.get("y", "a"), Some(Text::from("3")));
        assert_eq!(cfg.get("z", "p"), None);

        let sources = cfg.get_sources("z", "p");
        assert_eq!(sources.len(), 2);
        assert_eq!(sources[0].value(), &Some(Text::from("4")));
        assert_eq!(sources[1].value(), &None);
        assert_eq!(sources[0].source(), &"set4");
        assert_eq!(sources[1].source(), &"set5");
        assert_eq!(sources[0].location(), None);
        assert_eq!(sources[1].location(), None);
        assert_eq!(sources[1].file_content(), None);
    }

    #[test]
    fn test_keys() {
        let mut cfg = ConfigSet::new();
        cfg.set("foo", "other", Some(""), &"".into());
        cfg.set("foo", "bar", Some(""), &"".into());
        cfg.set("foo", "bar.baz", Some(""), &"".into());
        cfg.set("foo", "bar.qux", Some(""), &"".into());
        cfg.set("foo", "bar.qux.more", Some(""), &"".into());

        assert_eq!(
            cfg.keys("foo"),
            vec!["other", "bar", "bar.baz", "bar.qux", "bar.qux.more"]
        );
    }

    #[test]
    fn test_clone() {
        let mut cfg = ConfigSet::new();
        assert!(cfg.clone().sections().is_empty());
        cfg.set("x", "a", Some("1"), &"set1".into());
        assert_eq!(cfg.clone().sections(), vec![Text::from("x")]);
        assert_eq!(cfg.clone().get("x", "a"), Some("1".into()));
    }

    #[test]
    fn test_parse_basic() {
        let mut cfg = ConfigSet::new();
        cfg.parse(
            "[y]\n\
             a = 0\n\
             b=1\n\
             # override a to 2\n\
             a  =  2 \n\
             \n\
             [x]\n\
             m = this\n \
             value has\r\n \
             multi lines\n\
             ; comment again\n\
             n =\n",
            &"test_parse_basic".into(),
        );

        assert_eq!(cfg.sections(), vec![Text::from("y"), Text::from("x")]);
        assert_eq!(cfg.keys("y"), vec![Text::from("a"), Text::from("b")]);
        assert_eq!(cfg.keys("x"), vec![Text::from("m"), Text::from("n")]);

        assert_eq!(cfg.get("y", "a"), Some(Text::from("2")));
        assert_eq!(cfg.get("y", "b"), Some(Text::from("1")));
        assert_eq!(cfg.get("x", "n"), Some(Text::new()));
        assert_eq!(
            cfg.get("x", "m"),
            Some(Text::from("this\nvalue has\nmulti lines"))
        );

        let sources = cfg.get_sources("y", "a");
        assert_eq!(sources.len(), 2);
        assert_eq!(sources[0].value(), &Some(Text::from("0")));
        assert_eq!(sources[1].value(), &Some(Text::from("2")));
        assert_eq!(sources[0].source(), &"test_parse_basic");
        assert_eq!(sources[1].source(), &"test_parse_basic");
        assert_eq!(sources[0].location().unwrap(), (PathBuf::new(), 8..9));
        assert_eq!(sources[1].location().unwrap(), (PathBuf::new(), 38..39));
        assert_eq!(sources[1].file_content().unwrap().len(), 100);
    }

    #[test]
    fn test_parse_spaces() {
        let mut cfg = ConfigSet::new();

        cfg.parse(
            "# space after section name\n\
             [a]    \n\
             # empty lines\n    \n\t\n\n\
             x=1\n\
             # space in config name\n\
             y y \t =2\n\
             # space in multi-line config value, with trailing spaces\n\
             z=\t \n 3 3 \n  \n  4  \n\t5  \n  \n\
             # empty values\n\
             e1 =\n\
             e2 = \n\
             e3 =\n  \n\
             \n\
             # space in section name\n\
             [ b c\t]\n\
             # space in unset\n\
             y y =\n\
             %unset  y y \n\
             # no space at EOF\n\
             x=4",
            &"".into(),
        );

        assert_eq!(cfg.get("a", "x"), Some("1".into()));
        assert_eq!(cfg.get("a", "y y"), Some("2".into()));
        assert_eq!(cfg.get("a", "z"), Some("\n3 3\n\n4\n5".into()));
        assert_eq!(cfg.get("a", "e1"), Some("".into()));
        assert_eq!(cfg.get("a", "e2"), Some("".into()));
        assert_eq!(cfg.get("a", "e3"), Some("".into()));
        assert_eq!(cfg.get("b c", "y y"), None);
        assert_eq!(cfg.get("b c", "x"), Some("4".into()));
    }

    #[test]
    fn test_corner_cases() {
        let mut cfg = ConfigSet::new();
        let errors = cfg.parse(
            "# section looks like a config assignment\n\
             [a=b]\n\
             # comments look like config assignments\n\
             # a = b\n\
             ; a = b\n\
             # multiple equal signs in a config assignment\n\
             c = d = e\n\
             #",
            &"".into(),
        );

        assert_eq!(format!("{:?}", errors), "[]");
        assert_eq!(cfg.get("a=b", "c"), Some("d = e".into()));
        assert_eq!(cfg.get("a=b", "a"), None);
        assert_eq!(cfg.get("a=b", "# a"), None);
        assert_eq!(cfg.get("a=b", "; a"), None);
    }

    #[test]
    fn test_parse_errors() {
        let mut cfg = ConfigSet::new();
        let errors = cfg.parse("=foo", &"test_parse_errors".into());
        assert_eq!(format!("{}", errors[0]), "\"\":\nline 1: empty config name");

        let errors = cfg.parse(" a=b", &"test_parse_errors".into());
        assert_eq!(
            format!("{}", errors[0]),
            "\"\":\nline 1: indented line is not part of a multi-line config"
        );

        let errors = cfg.parse("%unset =foo", &"test_parse_errors".into());
        assert_eq!(
            format!("{}", errors[0]),
            "\"\":\nline 1: config name cannot include '='"
        );

        let errors = cfg.parse("[", &"test_parse_errors".into());
        assert_eq!(
            format!("{}", errors[0]),
            "\"\":\nline 1: missing ']' for section header"
        );

        let errors = cfg.parse("[]", &"test_parse_errors".into());
        assert_eq!(
            format!("{}", errors[0]),
            "\"\":\nline 1: empty section name"
        );

        let errors = cfg.parse("[a]]", &"test_parse_errors".into());
        assert_eq!(
            format!("{}", errors[0]),
            "\"\":\nline 1: extra content after section header"
        );

        let errors = cfg.parse("# foo\n[y", &"test_parse_errors".into());
        assert_eq!(
            format!("{}", errors[0]),
            "\"\":\nline 2: missing ']' for section header"
        );

        let mut cfg = ConfigSet::new();
        let errors = cfg.parse("\n\n%unknown", &"test_parse_errors".into());
        assert_eq!(
            format!("{}", errors[0]),
            "\"\":\nline 3: unknown directive (expect '%include' or '%unset')"
        );

        let mut cfg = ConfigSet::new();
        let errors = cfg.parse("[section]\nabc", &"test_parse_errors".into());
        assert_eq!(
            format!("{}", errors[0]),
            "\"\":\nline 2: expect '[section]' or 'name = value'"
        );
    }

    #[test]
    fn test_parse_unset() {
        let mut cfg = ConfigSet::new();
        cfg.parse(
            "[x]\n\
             a = 1\n\
             %unset b\n\
             b = 2\n\
             %unset  a \n\
             c = 3\n\
             d = 4\n\
             [y]\n\
             %unset  c\n\
             [x]\n\
             %unset  d ",
            &"test_parse_unset".into(),
        );

        assert_eq!(cfg.get("x", "a"), None);
        assert_eq!(cfg.get("x", "b"), Some(Text::from("2")));
        assert_eq!(cfg.get("x", "c"), Some(Text::from("3")));
        assert_eq!(cfg.get("x", "d"), None);
        assert_eq!(cfg.get_considering_unset("x", "d"), Some(None));
        assert_eq!(cfg.get_considering_unset("x", "e"), None);

        let sources = cfg.get_sources("x", "a");
        assert_eq!(sources.len(), 2);
        assert_eq!(sources[0].location().unwrap(), (PathBuf::new(), 8..9));
        assert_eq!(sources[1].location().unwrap(), (PathBuf::new(), 33..34));
    }

    #[test]
    fn test_filters() {
        fn exclude_list_section_x(
            section: Text,
            name: Text,
            value: Option<Text>,
        ) -> Option<(Text, Text, Option<Text>)> {
            if section.as_ref() == "x" {
                None
            } else {
                Some((section, name, value))
            }
        }

        fn swap_name_value(
            section: Text,
            name: Text,
            value: Option<Text>,
        ) -> Option<(Text, Text, Option<Text>)> {
            Some((section, value.unwrap(), name.into()))
        }

        fn rename_section_to_z(
            _section: Text,
            name: Text,
            value: Option<Text>,
        ) -> Option<(Text, Text, Option<Text>)> {
            Some(("z".into(), name, value))
        }

        let mut cfg = ConfigSet::new();
        let opts = Options::new()
            .append_filter(Box::new(exclude_list_section_x))
            .append_filter(Box::new(swap_name_value))
            .append_filter(Box::new(rename_section_to_z));
        cfg.parse(
            "[x]\n\
             a=1\n\
             [y]\n\
             b=c",
            &opts,
        );
        assert_eq!(cfg.get("x", "a"), None);
        assert_eq!(cfg.get("y", "b"), None);
        assert_eq!(cfg.get("z", "c"), Some(Text::from("b")));
    }

    pub(crate) fn write_file(path: PathBuf, content: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut f = fs::File::create(path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
    }

    #[test]
    fn test_parse_include() {
        let dir = TempDir::with_prefix("test_parse_include.").unwrap();
        write_file(
            dir.path().join("rootrc"),
            "[x]\n\
             b=1\n\
             a=1\n\
             %include dir/abc.rc\n\
             %include dir/y.rc\n\
             %include dir/loop.rc\n\
             %include b.rc\n\
             [y]\n\
             b=1\n\
             [x]\n\
             %unset f",
        );

        write_file(dir.path().join("dir/abc.rc"), "[x]\na=2\nb=2");
        write_file(dir.path().join("dir/y.rc"), "[y]\ny=1\n%include ../e.rc");
        write_file(dir.path().join("dir/loop.rc"), "%include ../rootrc");

        // Won't be loaded before it's not inside dir/ directly.
        write_file(dir.path().join("dir/unused/unused.rc"), "[unused]\na=1");

        // Won't be loaded before it does not have ".rc" extension.
        write_file(dir.path().join("dir/unusedrc"), "[unused]\na=1");

        // Will be loaded. `%include` shouldn't cause cycles.
        write_file(
            dir.path().join("b.rc"),
            "[x]\nb=4\n\
             %include dir/abc.rc\n\
             %include dir/y.rc\n\
             %include dir/loop.rc",
        );

        // Will be loaded. Shouldn't cause cycles.
        write_file(dir.path().join("e.rc"), "[x]\ne=e\n%include f.rc");
        write_file(
            dir.path().join("f.rc"),
            "[x]\nf=f\n%include e.rc\n%include rootrc",
        );

        let mut cfg = ConfigSet::new();
        let errors = cfg.load_path(dir.path().join("rootrc"), &"test_parse_include".into());
        assert!(errors.is_empty());

        assert_eq!(cfg.sections(), vec![Text::from("x"), Text::from("y")]);
        assert_eq!(
            cfg.keys("x"),
            vec![
                Text::from("b"),
                Text::from("a"),
                Text::from("e"),
                Text::from("f"),
            ]
        );
        assert_eq!(cfg.get("x", "a"), Some(Text::from("2")));
        assert_eq!(cfg.get("x", "b"), Some(Text::from("4")));
        assert_eq!(cfg.get("x", "e"), Some(Text::from("e")));
        assert_eq!(cfg.get("x", "f"), None);
        assert_eq!(cfg.get("y", "b"), Some(Text::from("1")));
    }

    #[test]
    fn test_parse_include_builtin() {
        let dir = TempDir::with_prefix("test_parse_include.").unwrap();
        write_file(dir.path().join("rootrc"), "%include builtin:git.rc\n");

        let mut cfg = ConfigSet::new();
        let errors = cfg.load_path(
            dir.path().join("rootrc"),
            &"test_parse_include_builtin".into(),
        );
        assert!(errors.is_empty());
    }

    #[test]
    fn test_parse_include_expand() {
        use std::env;
        // TODO: Audit that the environment access only happens in single-threaded code.
        unsafe { env::set_var("FOO", "f") };

        let dir = TempDir::with_prefix("test_parse_include_expand.").unwrap();
        write_file(
            dir.path().join("rootrc"),
            "%include ./${FOO}1/$FOO/3.rc\n\
             %include ./%FOO%2/%FOO%/4.rc\n",
        );

        write_file(dir.path().join("f1/f/3.rc"), "[x]\na=1\n");
        write_file(dir.path().join("f2/f/4.rc"), "[y]\nb=2\n");

        let mut cfg = ConfigSet::new();
        let errors = cfg.load_path(dir.path().join("rootrc"), &"include_expand".into());
        assert!(errors.is_empty());

        assert_eq!(cfg.get("x", "a"), Some(Text::from("1")));
        assert_eq!(cfg.get("y", "b"), Some(Text::from("2")));
    }

    #[test]
    fn test_named() {
        let mut cfg = ConfigSet::new();
        assert_eq!(cfg.layer_name(), "ConfigSet");
        cfg = cfg.named("foo");
        assert_eq!(cfg.layer_name(), "foo");
    }

    #[test]
    fn test_serialize() {
        let mut cfg = ConfigSet::new();
        let errors = cfg.parse(
            "[section_one]
normal=normal_value
space key=space value
newline=new \n line
unset_me=foo
%unset unset_me

[section_two]
empty=
list=value1,value2,value3
space_list=value1.a value1.b
    value2.a value2.b
",
            &"".into(),
        );
        assert!(errors.is_empty(), "cfg.parse had errors {:?}", errors);

        let serialized = cfg.to_string();
        assert_eq!(
            serialized,
            r#"[section_one]
normal=normal_value
space key=space value
newline=new
 line
%unset unset_me

[section_two]
empty=
list=value1,value2,value3
space_list=value1.a value1.b
 value2.a value2.b

"#
        );

        // Verify it round trips
        let mut cfg2 = ConfigSet::new();
        let errors = cfg2.parse(serialized, &"".into());
        assert!(errors.is_empty(), "cfg2.parse had errors {:?}", errors);
        assert_eq!(cfg.sections(), cfg2.sections());
    }

    #[test]
    fn test_secondary() {
        let mut cfg1 = ConfigSet::new();
        let mut cfg2 = ConfigSet::new();

        cfg1.parse(
            r#"[b]
x = 1
[d]
y = 1
%unset x
[a]
x = 1
y = 1
"#,
            &"test1".into(),
        );
        cfg2.parse(
            r#"[a]
z = 2
x = 2
[d]
x = 2
%unset z
%unset y
"#,
            &"test2".into(),
        );

        let mut config = cfg1.clone();
        config.secondary(Arc::new(cfg2));

        // section order: a, d (cfg2), b
        // name order: a.z, a.x (cfg2), a.y (cfg1); d.x, d.z, d.y (cfg2); b.x (cfg1)
        // override: cfg1 overrides cfg2; d.x, d.y, a.x
        // %unset in cfg1 and cfg2 is preserved
        assert_eq!(
            config.to_string(),
            "[a]\nz=2\nx=1\ny=1\n\n[d]\n%unset x\n%unset z\ny=1\n\n[b]\nx=1\n\n"
        );

        assert_eq!(config.sections().into_owned(), ["a", "d", "b"]);
        assert_eq!(config.keys("a"), ["z", "x", "y"]);
        assert_eq!(config.keys("d"), ["x", "z", "y"]);
        assert_eq!(config.keys("b"), ["x"]);
        assert_eq!(config.get("a", "x").unwrap(), "1");
        assert_eq!(config.get("d", "x"), None);
        assert_eq!(config.get("d", "y").unwrap(), "1");
        assert_eq!(config.get("d", "k"), None);
        assert_eq!(config.get_considering_unset("d", "x"), Some(None));
        assert_eq!(config.get_considering_unset("d", "k"), None);
        assert_eq!(
            config
                .get_sources("a", "x")
                .iter()
                .map(|s| s.source.to_string())
                .collect::<Vec<_>>(),
            ["test2", "test1"]
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
        assert!(cfg.get_or("foo", "bool1", || false).unwrap());
        assert_eq!(
            format!("{}", cfg.get_or("foo", "bool2", || true).unwrap_err()),
            "config foo.bool2 is invalid: invalid bool: unknown"
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

        assert!(cfg.get_or_default::<bool>("foo", "bool1").unwrap());
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

    #[test]
    fn test_pinned() {
        let mut cfg = ConfigSet::new();

        let pin = Options {
            pin: Some(true),
            ..Default::default()
        };
        cfg.set("shared_sec", "value", Some("pin"), &pin);
        cfg.set("pin_sec", "value", Some("pin"), &pin);

        let dont_pin = Options {
            pin: Some(false),
            ..Default::default()
        };
        cfg.set("shared_sec", "value", Some("main"), &dont_pin);
        cfg.set("main_sec", "value", Some("main"), &dont_pin);

        assert_eq!(cfg.sections(), vec!["shared_sec", "pin_sec", "main_sec"]);
        assert_eq!(cfg.keys("main_sec"), vec!["value"]);
        assert_eq!(cfg.keys("pin_sec"), vec!["value"]);
        assert_eq!(cfg.keys("shared_sec"), vec!["value"]);

        assert_eq!(cfg.get("shared_sec", "value"), Some("pin".into()));

        let sources = cfg.get_sources("shared_sec", "value");
        assert_eq!(sources.len(), 2);

        cfg.clear_unpinned();

        assert_eq!(cfg.sections(), vec!["shared_sec", "pin_sec"]);
        assert_eq!(cfg.keys("shared_sec"), vec!["value"]);

        let sources = cfg.get_sources("shared_sec", "value");
        assert_eq!(sources.len(), 1);
    }

    #[test]
    fn test_wrap_maintains_pinned_config() {
        let mut cfg = ConfigSet::new();

        cfg.set(
            "pinned",
            "pinned",
            Some("pinned"),
            &Options::default().pin(true),
        );

        cfg.set(
            "not-pinned",
            "not-pinned",
            Some("not-pinned"),
            &Options::default().pin(false),
        );

        let mut wrapped = ConfigSet::wrap(Arc::new(cfg));

        assert_eq!(wrapped.get("pinned", "pinned"), Some("pinned".into()));
        assert_eq!(
            wrapped.get("not-pinned", "not-pinned"),
            Some("not-pinned".into())
        );

        wrapped.clear_unpinned();

        assert_eq!(wrapped.get("pinned", "pinned"), Some("pinned".into()));
        assert_eq!(wrapped.get("not-pinned", "not-pinned"), None);
    }
}
