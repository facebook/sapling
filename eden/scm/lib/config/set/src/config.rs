/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::borrow::Cow;
use std::collections::HashSet;
use std::fs;
use std::hash::Hash;
use std::path::Path;
use std::path::PathBuf;
use std::str;
use std::sync::Arc;

use configmodel::Config;
pub use configmodel::ValueLocation;
pub use configmodel::ValueSource;
use hgrc_parser::parse;
use hgrc_parser::Instruction;
use indexmap::IndexMap;
use indexmap::IndexSet;
use minibytes::Text;
use util::path::expand_path;

use crate::error::Error;

/// Collection of config sections loaded from various sources.
#[derive(Clone, Default)]
pub struct ConfigSet {
    name: Text,
    sections: IndexMap<Text, Section>,
    // Canonicalized files that were loaded, including files with errors
    files: Vec<PathBuf>,
    // Secondary, immutable config to try out if `sections` does not
    // contain the requested config.
    secondary: Option<Arc<dyn Config>>,
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
    filters: Vec<Arc<Box<dyn Fn(Text, Text, Option<Text>) -> Option<(Text, Text, Option<Text>)>>>>,
}

impl Config for ConfigSet {
    /// Get config names under a section, sorted by insertion order.
    ///
    /// keys("foo") returns keys in section "foo".
    fn keys(&self, section: &str) -> Vec<Text> {
        let self_keys = self
            .sections
            .get(section)
            .map(|section| section.items.keys().cloned().collect())
            .unwrap_or_default();
        if let Some(secondary) = &self.secondary {
            let secondary_keys = secondary.keys(section);
            let result = merge_cow_list(Cow::Owned(secondary_keys), Cow::Owned(self_keys));
            result.into_owned()
        } else {
            self_keys
        }
    }

    /// Get config value for a given config.
    /// Return `None` if the config item does not exist.
    /// Return `Some(None)` if the config is is unset.
    fn get_considering_unset(&self, section: &str, name: &str) -> Option<Option<Text>> {
        let self_value = (|| -> Option<Option<Text>> {
            let section = self.sections.get(section)?;
            let value_sources: &Vec<ValueSource> = section.items.get(name)?;
            let value = value_sources.last()?.value.clone();
            Some(value)
        })();
        if let (None, Some(secondary)) = (&self_value, &self.secondary) {
            return secondary.get_considering_unset(section, name);
        }
        self_value
    }

    /// Get config sections.
    fn sections(&self) -> Cow<[Text]> {
        let sections = self.sections.keys().cloned().collect();
        let self_sections: Cow<[Text]> = Cow::Owned(sections);
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
    /// Return an emtpy vector if the config does not exist.
    fn get_sources(&self, section: &str, name: &str) -> Cow<[ValueSource]> {
        let self_sources: Cow<[ValueSource]> = match self
            .sections
            .get(section)
            .and_then(|section| section.items.get(name))
        {
            None => Cow::Owned(Vec::new()),
            Some(sources) => Cow::Borrowed(sources),
        };
        if let Some(secondary) = &self.secondary {
            let secondary_sources = secondary.get_sources(section, name);
            if secondary_sources.is_empty() {
                self_sources
            } else if self_sources.is_empty() {
                secondary_sources
            } else {
                let sources: Vec<ValueSource> = secondary_sources
                    .into_owned()
                    .into_iter()
                    .chain(self_sources.into_owned())
                    .collect();
                Cow::Owned(sources)
            }
        } else {
            self_sources
        }
    }

    /// Get on-disk files loaded for this `Config`.
    fn files(&self) -> Cow<[PathBuf]> {
        let self_files: Cow<[PathBuf]> = Cow::Borrowed(&self.files);
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
            secondary.layers()
        } else {
            Vec::new()
        }
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
        let result: IndexSet<T> = a.into_owned().into_iter().chain(b.into_owned()).collect();
        let result: Vec<T> = result.into_iter().collect();
        Cow::Owned(result)
    }
}

impl ConfigSet {
    /// Return an empty `ConfigSet`.
    pub fn new() -> Self {
        Default::default()
    }

    /// Attach a secondary config as fallback for items missing from the
    /// main config.
    ///
    /// The secondary config is immutable, is cheaper to clone, and provides
    /// the layers information.
    ///
    /// If a secondary config was already attached, it will be completed
    /// replaced by this call.
    pub fn secondary(&mut self, secondary: Arc<dyn Config>) -> &mut Self {
        self.secondary = Some(secondary);
        self
    }

    /// Update the name of the `ConfigSet`.
    pub fn named(&mut self, name: &str) -> &mut Self {
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
    /// Return a list of errors. An error pasing a file will stop that file from loading, without
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
        self.set_internal(section, name, value, None, &opts)
    }

    fn set_internal(
        &mut self,
        section: Text,
        name: Text,
        value: Option<Text>,
        location: Option<ValueLocation>,
        opts: &Options,
    ) {
        let filtered = opts
            .filters
            .iter()
            .fold(Some((section, name, value)), move |acc, func| {
                acc.and_then(|(section, name, value)| func(section, name, value))
            });
        if let Some((section, name, value)) = filtered {
            self.sections
                .entry(section)
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
        if let Ok(path) = path.canonicalize() {
            let path = &path;
            debug_assert!(path.is_absolute());

            if !visited.insert(path.to_path_buf()) {
                // skip - visited before
                return;
            }

            self.files.push(path.to_path_buf());

            match fs::read_to_string(path) {
                Ok(mut text) => {
                    text.push('\n');
                    let text = Text::from(text);
                    self.load_file_content(path, text, opts, visited, errors);
                }
                Err(error) => errors.push(Error::Io(path.to_path_buf(), error)),
            }
        } else {
            // On Windows, a UNC path `\\?\C:\foo\.\x` will fail to canonicalize
            // because it contains `.`. That path can be constructed by using
            // `PathBuf::join` to concatenate a UNC path `\\?\C:\foo` with
            // a "normal" path `.\x`.
            // Try to fix it automatically by stripping the UNC prefix and retry
            // `canonicalize`. `C:\foo\.\x` would be canonicalized without errors.
            #[cfg(windows)]
            {
                if let Some(path_str) = path.to_str() {
                    if path_str.starts_with(r"\\?\") {
                        let path = Path::new(&path_str[4..]);
                        self.load_file(&path, opts, visited, errors);
                    }
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
                            let text = Text::from(content);
                            let path = Path::new(include_path);
                            self.load_file_content(path, text, opts, visited, errors);
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

    pub fn files(&self) -> &[PathBuf] {
        &self.files
    }

    pub fn to_string(&self) -> String {
        let mut result = String::new();

        for section in self.sections().iter() {
            result.push('[');
            result.push_str(section.as_ref());
            result.push_str("]\n");

            for key in self.keys(section).iter() {
                let value = self.get_considering_unset(section, key);
                #[cfg(test)]
                {
                    let values = self.get_sources(section, key);
                    assert_eq!(values.last().map(|v| v.value().clone()), value);
                }
                if let Some(value) = value {
                    if let Some(value) = value {
                        result.push_str(key);
                        result.push('=');
                        // When a newline delimited list is loaded, the whitespace around each
                        // entry is trimmed. In order for the serialized config to be parsable, we
                        // need some indentation after each newline. Since this whitespace will be
                        // stripped on load, it shouldn't hurt anything.
                        let value = value.replace("\n", "\n ");
                        result.push_str(&value);
                        result.push('\n');
                    } else {
                        // None indicates the value was unset.
                        result.push_str("%unset ");
                        result.push_str(key);
                        result.push('\n');
                    }
                }
            }

            result.push_str("\n");
        }

        result
    }

    /// Drop configs from sources that are outside `allowed_locations` or
    /// `allowed_configs`.
    ///
    /// This function is being removed but we need logging to understand its
    /// side-effect.
    pub fn ensure_location_supersets(
        &mut self,
        allowed_locations: Option<HashSet<&str>>,
        allowed_configs: Option<HashSet<(&str, &str)>>,
    ) {
        for (sname, section) in self.sections.iter_mut() {
            for (kname, values) in section.items.iter_mut() {
                let values_copy = values.clone();
                let mut removals = 0;
                // value with a larger index takes precedence.
                for (index, value) in values_copy.iter().enumerate() {
                    // Convert the index into the original index.
                    let index = index - removals;

                    // Get the filename of the value's rc location
                    let path: PathBuf = match value.location() {
                        None => continue,
                        Some((path, _)) => path,
                    };
                    let location: Option<String> = path
                        .file_name()
                        .and_then(|f| f.to_str())
                        .map(|s| s.to_string());
                    // If only certain locations are allowed, and this isn't one of them, remove
                    // it. If location is None, it came from inmemory, so don't filter it.
                    if let Some(location) = location {
                        if crate::builtin::get(location.as_str()).is_none()
                            && allowed_locations
                                .as_ref()
                                .map(|a| a.contains(location.as_str()))
                                == Some(false)
                            && allowed_configs
                                .as_ref()
                                .map(|a| a.contains(&(sname, kname)))
                                != Some(true)
                        {
                            tracing::trace!(
                                target: "configset::validate",
                                "dropping {}.{} set by {} ({})",
                                sname.as_ref(),
                                kname.as_ref(),
                                path.display().to_string(),
                                value
                                    .value()
                                    .as_ref()
                                    .map(|v| v.as_ref())
                                    .unwrap_or_default(),
                            );
                            values.remove(index);
                            removals += 1;
                            continue;
                        }
                    }
                }

                // If the removal changes the config, log it as mismatched.
                if let (Some(before_remove), Some(after_remove)) =
                    (values_copy.last(), values.last())
                {
                    if before_remove.value != after_remove.value {
                        let source = match before_remove.location() {
                            None => before_remove.source().to_string(),
                            Some(l) => l.0.display().to_string(),
                        };
                        tracing::info!(
                             target: "config_mismatch",
                             config=&format!("{sname}.{kname}"),
                             expected=after_remove.value.clone().unwrap_or_default().as_ref(),
                             actual=before_remove.value.clone().unwrap_or_default().as_ref(),
                             source=source,
                        );
                    }
                }
            }
        }
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
        self.filters.push(Arc::new(filter));
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
    use tempdir::TempDir;

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
            Some(Text::from(&"this\nvalue has\nmulti lines"[..]))
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
        let dir = TempDir::new("test_parse_include").unwrap();
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
        let dir = TempDir::new("test_parse_include").unwrap();
        write_file(dir.path().join("rootrc"), "%include builtin:git.rc\n");

        let mut cfg = ConfigSet::new();
        let errors = cfg.load_path(
            dir.path().join("rootrc"),
            &"test_parse_include_builtin".into(),
        );
        assert!(errors.is_empty());

        assert_eq!(cfg.get("remotenames", "hoist"), Some(Text::from("remote")));
    }

    #[test]
    fn test_parse_include_expand() {
        use std::env;
        env::set_var("FOO", "f");

        let dir = TempDir::new("test_parse_include_expand").unwrap();
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
        cfg.named("foo");
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
    fn test_allowed_locations() {
        let mut cfg = ConfigSet::new();

        fn set(
            cfg: &mut ConfigSet,
            section: &'static str,
            key: &'static str,
            value: &'static str,
            location: &'static str,
        ) {
            cfg.set_internal(
                Text::from_static(section),
                Text::from_static(key),
                Some(Text::from_static(value)),
                Some(ValueLocation {
                    path: Arc::new(Path::new(location).to_owned()),
                    content: Text::from_static(""),
                    location: 0..1,
                }),
                &Options::new().source(Text::from_static("source")),
            );
        }

        set(&mut cfg, "section1", "key1", "value1", "subset1");
        set(&mut cfg, "section2", "key2", "value2", "subset2");

        let mut allow_list = HashSet::new();
        allow_list.insert("subset1");

        cfg.ensure_location_supersets(Some(allow_list.clone()), None);
        assert_eq!(
            cfg.get("section1", "key1"),
            Some(Text::from_static("value1"))
        );
        assert_eq!(cfg.get("section2", "key2"), None);

        // Check that allow_configs allows the config through, even if allow_locations did not.
        let mut allow_configs = HashSet::new();
        allow_configs.insert(("section2", "key2"));

        set(&mut cfg, "section2", "key2", "value2", "subset2");
        cfg.ensure_location_supersets(Some(allow_list), Some(allow_configs));
        assert_eq!(
            cfg.get("section1", "key1"),
            Some(Text::from_static("value1"))
        );
        assert_eq!(
            cfg.get("section2", "key2"),
            Some(Text::from_static("value2"))
        );
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
    fn test_verifier_removal() {
        let mut cfg = ConfigSet::new();

        fn set(
            cfg: &mut ConfigSet,
            section: &'static str,
            key: &'static str,
            value: &'static str,
            location: &'static str,
        ) {
            cfg.set_internal(
                Text::from_static(section),
                Text::from_static(key),
                Some(Text::from_static(value)),
                Some(ValueLocation {
                    path: Arc::new(Path::new(location).to_owned()),
                    content: Text::from_static(""),
                    location: 0..1,
                }),
                &Options::new().source(Text::from_static("source")),
            );
        }

        // This test verifies that allowed location removal and subset removal interact nicely
        // together.
        set(&mut cfg, "section", "key", "value", "subset");
        set(&mut cfg, "section", "key", "value2", "super");

        let mut allowed_locations = HashSet::new();
        allowed_locations.insert("super");

        cfg.ensure_location_supersets(Some(allowed_locations), None);
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
