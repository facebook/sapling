use bytes::Bytes;
use error::Error;
use linked_hash_map::LinkedHashMap;
use std::collections::HashSet;
use std::convert::AsRef;
use std::ffi::OsStr;
use std::fs;
use std::io::Read;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Collection of config sections loaded from various sources.
#[derive(Default)]
pub struct ConfigSet {
    sections: LinkedHashMap<Bytes, Section>,
    errors: Vec<Error>,
}

/// Internal representation of a config section.
#[derive(Default)]
struct Section {
    items: LinkedHashMap<Bytes, Vec<ValueSource>>,
}

/// A config value with associated metadata like where it comes from.
#[derive(Clone)]
pub struct ValueSource {
    value: Option<Bytes>,
    source: Bytes, // global, user, repo, "--config", or an extension name, etc.
    location: Option<ValueLocation>,
}

/// The on-disk file name and byte offsets that provide the config value.
/// Useful if applications want to edit config values in-place.
#[derive(Clone)]
struct ValueLocation {
    path: Arc<PathBuf>,
    location: Range<usize>,
}

impl ConfigSet {
    /// Return an empty `ConfigSet`.
    pub fn new() -> Self {
        Default::default()
    }

    /// Load config files at given path. The path could be either a directory or a file.
    ///
    /// If `path` is a directory, files directly inside it with extension `.rc` will be loaded.
    /// Files in subdirectories are ignored. The order of loading them is undefined. If `path` is
    /// a file, it will be loaded directly.
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
    /// The `source` field is to extra information about who initialized the config loading. For
    /// example, "user_hgrc" indicates it is from user config file.
    ///
    /// Errors will be pushed to an internal array, and can be retrieved by `errors`. Non-existed
    /// path is not considered as an error.
    pub fn load_path<P: AsRef<Path>, S: Into<Bytes>>(&mut self, path: P, source: S) {
        let mut visited = HashSet::new();
        let source = source.into();
        self.load_dir_or_file(path.as_ref(), &source, &mut visited);
    }

    /// Load content of an unnamed config file. The `ValueLocation`s of loaded config items will
    /// have an empty `path`.
    ///
    /// The `source` field is to extra information about who initialized the config loading. For
    /// example, "--config" indicates it is from the global "--config" flag, "env" indicates
    /// it is from environment variables (ex. "PAGER").
    ///
    /// Errors will be pushed to an internal array, and can be retrieved by `errors`.
    pub fn parse<B: Into<Bytes>, S: Into<Bytes>>(&mut self, content: B, source: S) {
        let mut visited = HashSet::new();
        let buf = content.into();
        let source = source.into();
        self.load_file_content(Path::new(""), buf, &source, &mut visited);
    }

    /// Get config sections.
    pub fn sections(&self) -> Vec<Bytes> {
        self.sections.keys().cloned().collect()
    }

    /// Get config names in the given section. Sorted by insertion order.
    pub fn keys<S: Into<Bytes>>(&self, section: S) -> Vec<Bytes> {
        self.sections
            .get(&section.into())
            .map(|section| section.items.keys().cloned().collect())
            .unwrap_or(Vec::new())
    }

    /// Get config value for a given config.
    /// Return `None` if the config item does not exist or is unset.
    pub fn get<S: Into<Bytes>, N: Into<Bytes>>(&self, section: S, name: N) -> Option<Bytes> {
        self.sections.get(&section.into()).and_then(|section| {
            section
                .items
                .get(&name.into())
                .and_then(|values| values.last().and_then(|value| value.value.clone()))
        })
    }

    /// Get detailed sources of a given config, including overrides, and source information.
    /// The last item in the returned vector is the latest value that is considered effective.
    ///
    /// Return an emtpy vector if the config does not exist.
    pub fn get_sources<S: Into<Bytes>, N: Into<Bytes>>(
        &self,
        section: S,
        name: N,
    ) -> Vec<ValueSource> {
        self.sections
            .get(&section.into())
            .and_then(|section| section.items.get(&name.into()).map(|values| values.clone()))
            .unwrap_or(Vec::new())
    }

    /// Set a config item directly. `section`, `name` locates the config. `value` is the new value.
    /// `source` is some annotation about who set it, ex. "reporc", "userrc", "--config", etc.
    pub fn set<T: Into<Bytes>, N: Into<Bytes>, S: Into<Bytes>>(
        &mut self,
        section: T,
        name: N,
        value: Option<&[u8]>,
        source: S,
    ) {
        let section = section.into();
        let name = name.into();
        let source = source.into();
        let value = value.map(|v| Bytes::from(v));
        self.set_internal(section, name, value, None, &source)
    }

    /// Get errors caused by parsing config files previously.
    pub fn errors(&self) -> &Vec<Error> {
        &self.errors
    }

    fn set_internal(
        &mut self,
        section: Bytes,
        name: Bytes,
        value: Option<Bytes>,
        location: Option<ValueLocation>,
        source: &Bytes,
    ) {
        self.sections
            .entry(section)
            .or_insert_with(|| Default::default())
            .items
            .entry(name)
            .or_insert_with(|| Vec::with_capacity(1))
            .push(ValueSource {
                value,
                location,
                source: source.clone(),
            })
    }

    fn load_dir_or_file(&mut self, path: &Path, source: &Bytes, visited: &mut HashSet<PathBuf>) {
        if let Ok(path) = path.canonicalize() {
            if path.is_dir() {
                self.load_dir(&path, source, visited);
            } else {
                self.load_file(&path, source, visited);
            }
        }
        // If `path.canonicalize` reports an error. It's usually the path cannot
        // be resolved (ex. does not exist). It is considered normal and is not
        // reported in `self.errors`.
    }

    fn load_dir(&mut self, dir: &Path, source: &Bytes, visited: &mut HashSet<PathBuf>) {
        let rc_ext = OsStr::new("rc");
        match dir.read_dir() {
            Ok(entries) => for entry in entries {
                match entry {
                    Ok(entry) => {
                        let path = entry.path();
                        if path.is_file() && path.extension() == Some(rc_ext) {
                            self.load_file(&path, source, visited);
                        }
                    }
                    Err(error) => self.error(Error::Io(dir.to_path_buf(), error)),
                }
            },
            Err(error) => self.error(Error::Io(dir.to_path_buf(), error)),
        }
    }

    fn load_file(&mut self, path: &Path, source: &Bytes, visited: &mut HashSet<PathBuf>) {
        debug_assert!(path.is_absolute());

        if !visited.insert(path.to_path_buf()) {
            // skip - visited before
            return;
        }

        match fs::File::open(path) {
            Ok(mut file) => {
                let mut buf = Vec::with_capacity(256);
                if let Err(error) = file.read_to_end(&mut buf) {
                    self.error(Error::Io(path.to_path_buf(), error));
                    return;
                }
                buf.push(b'\n');
                let buf = Bytes::from(buf);

                self.load_file_content(path, buf, source, visited);
            }
            Err(error) => self.error(Error::Io(path.to_path_buf(), error)),
        }
    }

    fn load_file_content(
        &mut self,
        path: &Path,
        buf: Bytes,
        source: &Bytes,
        visited: &mut HashSet<PathBuf>,
    ) {
        let mut pos = 0;
        let mut section = Bytes::new();
        let shared_path = Arc::new(path.to_path_buf()); // use Arc to do shallow copy
        let skip_include = path.parent().is_none(); // skip handling %include if path is empty

        while pos < buf.len() {
            match buf[pos] {
                b'\n' | b'\r' => pos += 1,
                b'[' => {
                    let section_start = pos + 1;
                    match memchr(b']', &buf.as_ref()[section_start..]) {
                        Some(len) => {
                            let section_end = section_start + len;
                            section = strip(&buf, section_start, section_end);
                            pos = section_end + 1;
                        }
                        None => {
                            self.error(Error::Parse(
                                path.to_path_buf(),
                                pos,
                                "missing ']' for section name",
                            ));
                            return;
                        }
                    }
                }
                b';' | b'#' => {
                    match memchr(b'\n', &buf.as_ref()[pos..]) {
                        Some(len) => pos += len, // skip this line
                        None => return,          // reach file end
                    }
                }
                b'%' => {
                    static INCLUDE: &[u8] = b"%include ";
                    static UNSET: &[u8] = b"%unset ";
                    if buf.get(pos..pos + INCLUDE.len()) == Some(INCLUDE) {
                        let path_start = pos + INCLUDE.len();
                        let path_end = memchr(b'\n', &buf.as_ref()[pos..])
                            .map(|len| len + pos)
                            .unwrap_or(buf.len());
                        if !skip_include {
                            match ::std::str::from_utf8(&buf[path_start..path_end]) {
                                Ok(literal_include_path) => {
                                    let full_include_path = path.parent()
                                        .unwrap()
                                        .join(expand_path(literal_include_path));
                                    self.load_dir_or_file(&full_include_path, source, visited);
                                }
                                Err(_error) => {
                                    self.error(Error::Parse(
                                        path.to_path_buf(),
                                        path_start,
                                        "invalid utf-8",
                                    ));
                                }
                            }
                        }
                        pos = path_end;
                    } else if buf.get(pos..pos + UNSET.len()) == Some(UNSET) {
                        let name_start = pos + UNSET.len();
                        let name_end = memchr(b'\n', &buf.as_ref()[pos..])
                            .map(|len| len + pos)
                            .unwrap_or(buf.len());
                        let name = strip(&buf, name_start, name_end);
                        let location = ValueLocation {
                            path: shared_path.clone(),
                            location: pos..name_end,
                        };
                        self.set_internal(section.clone(), name, None, location.into(), source);
                        pos = name_end;
                    } else {
                        self.error(Error::Parse(path.to_path_buf(), pos, "unknown instruction"));
                        return;
                    }
                }
                _ => {
                    let name_start = pos;
                    match memchr(b'=', &buf.as_ref()[name_start..]) {
                        Some(len) => {
                            let equal_pos = name_start + len;
                            let name = strip(&buf, name_start, equal_pos);
                            // Find the end of value. It could be multi-line.
                            let value_start = equal_pos + 1;
                            let mut value_end = value_start;
                            loop {
                                match memchr(b'\n', &buf.as_ref()[value_end..]) {
                                    Some(len) => {
                                        value_end += len + 1;
                                        let next_line_first_char =
                                            *buf.get(value_end).unwrap_or(&b'.');
                                        if !is_space(next_line_first_char) {
                                            break;
                                        }
                                    }
                                    None => {
                                        value_end = buf.len();
                                        break;
                                    }
                                }
                            }
                            let (start, end) = strip_offsets(&buf, value_start, value_end);
                            let value = buf.slice(start, end);
                            let location = ValueLocation {
                                path: shared_path.clone(),
                                location: start..end,
                            };

                            self.set_internal(
                                section.clone(),
                                name,
                                value.into(),
                                location.into(),
                                source,
                            );
                            pos = value_end;
                        }
                        None => {
                            self.error(Error::Parse(
                                path.to_path_buf(),
                                pos,
                                "missing '=' for config value",
                            ));
                            return;
                        }
                    }
                }
            } // match buf[pos]
        }
    }

    #[inline]
    fn error(&mut self, error: Error) {
        self.errors.push(error);
    }
}

impl ValueSource {
    /// Return the actual value stored in this config value, or `None` if uset.
    pub fn value(&self) -> &Option<Bytes> {
        &self.value
    }

    /// Return the "source" information for the config value. It's usually who sets the config,
    /// like "--config", "user_hgrc", "system_hgrc", etc.
    pub fn source(&self) -> &Bytes {
        &self.source
    }

    /// Return the file path and byte range for the exact config value,
    /// or `None` if there is no such information.
    ///
    /// If the value is `None`, the byte range is for the "%unset" statement.
    pub fn location(&self) -> Option<(PathBuf, Range<usize>)> {
        match self.location {
            Some(ref src) => Some((src.path.as_ref().to_path_buf(), src.location.clone())),
            None => None,
        }
    }
}

/// C memchr-like API
#[inline]
fn memchr(needle: u8, haystack: &[u8]) -> Option<usize> {
    haystack.iter().position(|&x| x == needle)
}

/// Test if a binary char is a space.
#[inline]
fn is_space(byte: u8) -> bool {
    b" \t\r\n".contains(&byte)
}

/// Remove space characters from both ends. `start` position is inclusive, `end` is exclusive.
/// Return the stripped `start` and `end` offsets.
#[inline]
fn strip_offsets(buf: &Bytes, start: usize, end: usize) -> (usize, usize) {
    let mut start = start;
    let mut end = end;
    while start < end && is_space(buf[start]) {
        start += 1
    }
    while start < end && is_space(buf[end - 1]) {
        end -= 1
    }
    (start, end)
}

/// Strip spaces and return a `Bytes` sub-slice.
#[inline]
fn strip(buf: &Bytes, start: usize, end: usize) -> Bytes {
    let (start, end) = strip_offsets(buf, start, end);
    buf.slice(start, end)
}

/// Expand `~` to home directory.
fn expand_path(path: &str) -> PathBuf {
    if path.starts_with("~/") {
        // TODO(quark): migrate to dirs or shellexpand once tp2 has them.
        #[allow(deprecated)]
        let home_dir = ::std::env::home_dir();
        if let Some(home) = home_dir {
            return home.join(Path::new(&path[2..]));
        }
    }
    Path::new(path).to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempdir::TempDir;

    #[test]
    fn test_empty() {
        let cfg = ConfigSet::new();
        assert!(cfg.sections().is_empty());
        assert!(cfg.keys("foo").is_empty());
        assert!(cfg.get("foo", "bar").is_none());
        assert!(cfg.get_sources("foo", "bar").is_empty());
        assert!(cfg.errors().is_empty());
    }

    #[test]
    fn test_set() {
        let mut cfg = ConfigSet::new();
        cfg.set("y", "b", Some(b"1"), "set1");
        cfg.set("y", "b", Some(b"2"), "set2");
        cfg.set("y", "a", Some(b"3"), "set3");
        cfg.set("z", "p", Some(b"4"), "set4");
        cfg.set("z", "p", None, "set5");
        assert_eq!(cfg.sections(), vec![Bytes::from("y"), Bytes::from("z")]);
        assert_eq!(cfg.keys("y"), vec![Bytes::from("b"), Bytes::from("a")]);
        assert_eq!(cfg.get("y", "b"), Some(Bytes::from("2")));
        assert_eq!(cfg.get("y", "a"), Some(Bytes::from("3")));
        assert_eq!(cfg.get("z", "p"), None);

        let sources = cfg.get_sources("z", "p");
        assert_eq!(sources.len(), 2);
        assert_eq!(sources[0].value(), &Some(Bytes::from("4")));
        assert_eq!(sources[1].value(), &None);
        assert_eq!(sources[0].source(), "set4");
        assert_eq!(sources[1].source(), "set5");
        assert_eq!(sources[0].location(), None);
        assert_eq!(sources[1].location(), None);
    }

    #[test]
    fn test_parse_basic() {
        let mut cfg = ConfigSet::new();
        cfg.parse(
            "[y]\n\
             a = 0\n\
             b=1\n\
             # override a to 2
              a  =  2 \n\
             \n\
             [x]\n\
             m = this\n \
             value has\n \
             multi lines\n\
             ; comment again\n\
             n =",
            "test_parse_basic",
        );

        assert_eq!(cfg.sections(), vec![Bytes::from("y"), Bytes::from("x")]);
        assert_eq!(cfg.keys("y"), vec![Bytes::from("a"), Bytes::from("b")]);
        assert_eq!(cfg.keys("x"), vec![Bytes::from("m"), Bytes::from("n")]);

        assert_eq!(cfg.get("y", "a"), Some(Bytes::from("2")));
        assert_eq!(cfg.get("y", "b"), Some(Bytes::from("1")));
        assert_eq!(cfg.get("x", "n"), Some(Bytes::new()));
        assert_eq!(
            cfg.get("x", "m"),
            Some(Bytes::from(&b"this\n value has\n multi lines"[..]))
        );

        let sources = cfg.get_sources("y", "a");
        assert_eq!(sources.len(), 2);
        assert_eq!(sources[0].value(), &Some(Bytes::from("0")));
        assert_eq!(sources[1].value(), &Some(Bytes::from("2")));
        assert_eq!(sources[0].source(), "test_parse_basic");
        assert_eq!(sources[1].source(), "test_parse_basic");
        assert_eq!(sources[0].location().unwrap(), (PathBuf::new(), 8..9));
        assert_eq!(sources[1].location().unwrap(), (PathBuf::new(), 52..53));
    }

    #[test]
    fn test_parse_errors() {
        let mut cfg = ConfigSet::new();
        cfg.parse("# foo\n[y", "test_parse_errors");
        assert_eq!(
            format!("{}", cfg.errors()[0]),
            "\"\": parse error around byte 6: missing \']\' for section name"
        );

        let mut cfg = ConfigSet::new();
        cfg.parse("\n\n%unknown", "test_parse_errors");
        assert_eq!(
            format!("{}", cfg.errors()[0]),
            "\"\": parse error around byte 2: unknown instruction"
        );

        let mut cfg = ConfigSet::new();
        cfg.parse("[section]\nabc", "test_parse_errors");
        assert_eq!(
            format!("{}", cfg.errors()[0]),
            "\"\": parse error around byte 10: missing \'=\' for config value"
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
            "test_parse_unset",
        );

        assert_eq!(cfg.get("x", "a"), None);
        assert_eq!(cfg.get("x", "b"), Some(Bytes::from("2")));
        assert_eq!(cfg.get("x", "c"), Some(Bytes::from("3")));
        assert_eq!(cfg.get("x", "d"), None);

        let sources = cfg.get_sources("x", "a");
        assert_eq!(sources.len(), 2);
        assert_eq!(sources[0].location().unwrap(), (PathBuf::new(), 8..9));
        assert_eq!(sources[1].location().unwrap(), (PathBuf::new(), 25..35));
    }

    fn write_file(path: PathBuf, content: &str) {
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
             %include dir\n\
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
        write_file(dir.path().join("b.rc"), "[x]\nb=4\n%include dir");

        // Will be loaded. Shouldn't cause cycles.
        write_file(dir.path().join("e.rc"), "[x]\ne=e\n%include f.rc");
        write_file(
            dir.path().join("f.rc"),
            "[x]\nf=f\n%include e.rc\n%include rootrc",
        );

        let mut cfg = ConfigSet::new();
        cfg.load_path(dir.path().join("rootrc"), "test_parse_include");
        assert!(cfg.errors().is_empty());

        assert_eq!(cfg.sections(), vec![Bytes::from("x"), Bytes::from("y")]);
        assert_eq!(
            cfg.keys("x"),
            vec![
                Bytes::from("b"),
                Bytes::from("a"),
                Bytes::from("e"),
                Bytes::from("f"),
            ]
        );
        assert_eq!(cfg.get("x", "a"), Some(Bytes::from("2")));
        assert_eq!(cfg.get("x", "b"), Some(Bytes::from("4")));
        assert_eq!(cfg.get("x", "e"), Some(Bytes::from("e")));
        assert_eq!(cfg.get("x", "f"), None);
        assert_eq!(cfg.get("y", "b"), Some(Bytes::from("1")));
    }
}
