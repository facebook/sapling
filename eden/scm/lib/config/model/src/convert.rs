/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;
use std::time::Duration;

use minibytes::Text;
use util::path::expand_path;

use crate::Error;
use crate::Result;

pub trait FromConfigValue: Sized {
    fn try_from_str(s: &str) -> Result<Self>;
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

impl FromConfigValue for Duration {
    fn try_from_str(s: &str) -> Result<Self> {
        Ok(Duration::from_secs_f64(s.parse()?))
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
/// use configmodel::convert::parse_list;
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
    }

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
}
