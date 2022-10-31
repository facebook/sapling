/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::io::Write;

use serde::Serialize;
use serde_json::to_writer_pretty;

use crate::errors::FormatterNotFound;
use crate::errors::FormattingError;

pub type FormatResult<T> = std::result::Result<T, FormattingError>;

#[derive(Default)]
pub struct FormatOptions {
    pub debug: bool,
    pub verbose: bool,
    pub quiet: bool,
    pub color: bool,
    pub debug_color: bool,
}

pub trait JsonFormattable {
    fn format_json(&self, writer: &mut dyn Write) -> Result<(), serde_json::Error>;
}

impl<S: Serialize> JsonFormattable for S {
    fn format_json(&self, writer: &mut dyn Write) -> Result<(), serde_json::Error> {
        to_writer_pretty(writer, self)?;
        Ok(())
    }
}

pub trait StyleWrite: Write {
    fn write_styled(&mut self, style: &str, text: &str) -> anyhow::Result<()>;
}

struct PlainWriter<'a> {
    w: &'a mut dyn Write,
    styler: &'a mut termstyle::Styler,
    styles: &'a HashMap<String, String>,
    should_color: bool,
    debug: bool,
}

impl Write for PlainWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.w.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.w.flush()
    }
}

impl StyleWrite for PlainWriter<'_> {
    fn write_styled(&mut self, style: &str, mut text: &str) -> anyhow::Result<()> {
        if self.debug {
            let mut end = "";
            if let Some(stripped) = text.strip_suffix('\n') {
                text = stripped;
                end = "\n";
            }
            write!(self.w, "[{text}|{style}]{end}")?;
            return Ok(());
        }

        if !self.should_color {
            self.w.write_all(text.as_bytes())?;
            return Ok(());
        }

        let style = style
            .split_ascii_whitespace()
            .map(|s| self.styles.get(s).map_or(s, |s| s.as_ref()))
            .collect::<Vec<&str>>()
            .join(" ");
        self.styler.render(self.w, &style, text)?;
        Ok(())
    }
}

pub trait Formattable: JsonFormattable {
    fn format_plain(
        &self,
        options: &FormatOptions,
        writer: &mut dyn StyleWrite,
    ) -> Result<(), anyhow::Error>;
}

pub trait ListFormatter {
    fn format_item(&mut self, item: &dyn Formattable) -> FormatResult<()>;
    fn begin_list(&mut self) -> FormatResult<()>;
    fn end_list(&mut self) -> FormatResult<()>;
}

pub struct PlainFormatter {
    writer: Box<dyn Write>,
    options: FormatOptions,
    styles: HashMap<String, String>,
    styler: termstyle::Styler,
}

pub struct JsonFormatter {
    writer: Box<dyn Write>,
    first_item_formatted: bool,
}

impl ListFormatter for PlainFormatter {
    fn format_item(&mut self, item: &dyn Formattable) -> FormatResult<()> {
        item.format_plain(
            &self.options,
            &mut PlainWriter {
                w: self.writer.as_mut(),
                styler: &mut self.styler,
                styles: &self.styles,
                should_color: self.options.color,
                debug: self.options.debug_color,
            },
        )
        .map_err(|err| match err.downcast::<std::io::Error>() {
            Ok(io_err) => FormattingError::WriterError(io_err),
            Err(err) => FormattingError::PlainFormattingError(err),
        })
    }

    fn begin_list(&mut self) -> FormatResult<()> {
        Ok(())
    }

    fn end_list(&mut self) -> FormatResult<()> {
        Ok(())
    }
}

impl ListFormatter for JsonFormatter {
    fn format_item(&mut self, item: &dyn Formattable) -> FormatResult<()> {
        let prev_separator = if self.first_item_formatted {
            ","
        } else {
            self.first_item_formatted = true;
            ""
        };
        write!(self.writer, "{}\n", prev_separator)?;
        item.format_json(self.writer.as_mut())?;
        Ok(())
    }

    fn begin_list(&mut self) -> FormatResult<()> {
        write!(self.writer, "[")?;
        Ok(())
    }

    fn end_list(&mut self) -> FormatResult<()> {
        write!(self.writer, "\n]\n")?;
        Ok(())
    }
}

pub fn get_formatter(
    config: &dyn configmodel::Config,
    _topic: &str,
    template: &str,
    options: FormatOptions,
    writer: Box<dyn Write>,
) -> anyhow::Result<Box<dyn ListFormatter>> {
    match template {
        "" => {
            let styles: HashMap<String, String> = config
                .keys("color")
                .into_iter()
                .filter_map(|k| {
                    if !k.contains('.') || k.starts_with("color.") {
                        None
                    } else {
                        Some((
                            k.to_string(),
                            config.get("color", &k).unwrap_or_default().to_string(),
                        ))
                    }
                })
                .collect();

            Ok(Box::new(PlainFormatter {
                writer,
                options,
                styles,
                styler: termstyle::Styler::new()?,
            }))
        }
        "json" => Ok(Box::new(JsonFormatter {
            writer,
            first_item_formatted: false,
        })),
        _ => Err(FormatterNotFound(template.into()).into()),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::io::Result as IoResult;
    use std::rc::Rc;

    use anyhow::bail;
    use serde::Deserialize;
    use serde_json::json;

    use super::*;

    #[derive(Serialize, Deserialize)]
    struct RequestTest<'a> {
        url: &'a str,
        result: u32,
    }

    impl<'a> Formattable for RequestTest<'a> {
        fn format_plain(
            &self,
            _options: &FormatOptions,
            writer: &mut dyn StyleWrite,
        ) -> Result<(), anyhow::Error> {
            write!(writer, "{}: {}", self.url, self.result)?;
            Ok(())
        }
    }

    struct Buffer {
        writer: Rc<RefCell<Vec<u8>>>,
    }

    impl Write for Buffer {
        fn write(&mut self, buf: &[u8]) -> IoResult<usize> {
            self.writer.borrow_mut().write(buf)
        }

        fn flush(&mut self) -> IoResult<()> {
            self.writer.borrow_mut().flush()
        }
    }

    struct FaultyItem;

    impl Formattable for FaultyItem {
        fn format_plain(
            &self,
            _options: &FormatOptions,
            _writer: &mut dyn StyleWrite,
        ) -> Result<(), anyhow::Error> {
            bail!("Nope")
        }
    }

    impl JsonFormattable for FaultyItem {
        fn format_json(&self, _writer: &mut dyn Write) -> Result<(), serde_json::Error> {
            let serialized = json!({
                "x": 1,
                "y": 2,
            })
            .to_string();
            serde_json::from_str::<RequestTest>(serialized.as_str())?;
            Ok(())
        }
    }

    struct FaultyBuffer {
        buf: [u8; 0],
    }

    impl Write for FaultyBuffer {
        fn write(&mut self, buf: &[u8]) -> IoResult<usize> {
            (&mut self.buf[..]).write(buf)
        }

        fn flush(&mut self) -> IoResult<()> {
            (&mut self.buf[..]).flush()
        }
    }

    fn get_trivial_formatter(
        template: &str,
        buffer: Rc<RefCell<Vec<u8>>>,
    ) -> anyhow::Result<Box<dyn ListFormatter>> {
        let mut colors: BTreeMap<&str, &str> = BTreeMap::new();
        colors.insert("color.foo.bar", "green");

        get_formatter(
            &colors,
            "",
            template,
            FormatOptions {
                color: true,
                ..Default::default()
            },
            Box::new(Buffer { writer: buffer }),
        )
    }

    #[test]
    fn test_formatter() {
        let buf = Rc::new(RefCell::new(Vec::<u8>::new()));
        let err = get_trivial_formatter("{node|short}", buf.clone())
            .err()
            .unwrap();
        assert_eq!(
            err.to_string(),
            "unable to find formatter for template {node|short}"
        );

        let item = RequestTest {
            url: "foo://bar",
            result: 200,
        };
        let mut fm = get_trivial_formatter("", buf.clone()).unwrap();
        fm.format_item(&item).unwrap();
        assert_eq!(
            String::from_utf8(buf.as_ref().borrow().clone()).unwrap(),
            "foo://bar: 200".to_string()
        );
    }

    #[derive(Serialize, Deserialize)]
    struct ColorfulItem {}

    impl Formattable for ColorfulItem {
        fn format_plain(
            &self,
            _options: &FormatOptions,
            writer: &mut dyn StyleWrite,
        ) -> Result<(), anyhow::Error> {
            writer.write_all(b"no style\n")?;
            writer.write_styled("unknown-style", "unknown style\n")?;
            writer.write_styled("red", "red\n")?;
            writer.write_styled("foo.bar", "green\n")?;
            Ok(())
        }
    }

    #[test]
    fn test_colors() {
        let buf = Rc::new(RefCell::new(Vec::<u8>::new()));
        let mut fm = get_trivial_formatter("", buf.clone()).unwrap();
        fm.format_item(&ColorfulItem {}).unwrap();
        assert_eq!(
            String::from_utf8(buf.as_ref().borrow().clone()).unwrap(),
            "no style
unknown style
\x1b[31mred\x1b[39m
\x1b[32mgreen\x1b[39m
",
        );
    }

    #[test]
    fn test_json_formatter() {
        let item = RequestTest {
            url: "foo://bar",
            result: 200,
        };

        // Test no items
        let buf = Rc::new(RefCell::new(Vec::<u8>::new()));
        let mut fm = get_trivial_formatter("json", buf.clone()).unwrap();
        fm.begin_list().unwrap();
        fm.end_list().unwrap();
        assert_eq!(
            String::from_utf8(buf.as_ref().borrow().clone()).unwrap(),
            "[\n]\n".to_string()
        );
        // Test single item
        let buf = Rc::new(RefCell::new(Vec::<u8>::new()));
        let mut fm = get_trivial_formatter("json", buf.clone()).unwrap();
        fm.begin_list().unwrap();
        fm.format_item(&item).unwrap();
        fm.end_list().unwrap();
        assert_eq!(
            String::from_utf8(buf.as_ref().borrow().clone()).unwrap(),
            r#"[
{
  "url": "foo://bar",
  "result": 200
}
]
"#
            .to_string()
        );
        // Test more than one item
        let buf = Rc::new(RefCell::new(Vec::<u8>::new()));
        let mut fm = get_trivial_formatter("json", buf.clone()).unwrap();
        fm.begin_list().unwrap();
        fm.format_item(&item).unwrap();
        fm.format_item(&item).unwrap();
        fm.end_list().unwrap();
        assert_eq!(
            String::from_utf8(buf.as_ref().borrow().clone()).unwrap(),
            r#"[
{
  "url": "foo://bar",
  "result": 200
},
{
  "url": "foo://bar",
  "result": 200
}
]
"#
            .to_string()
        );
    }

    #[test]
    fn test_errors() {
        let buf: [u8; 0] = [0; 0];
        let mut fm = get_formatter(
            &BTreeMap::<&str, &str>::new(),
            "",
            "",
            FormatOptions::default(),
            Box::new(FaultyBuffer { buf }),
        )
        .unwrap();

        let item = RequestTest {
            url: "foo://bar",
            result: 200,
        };
        assert!(matches!(
            fm.format_item(&item).err().unwrap(),
            FormattingError::WriterError(_)
        ));

        let item = FaultyItem;
        assert!(matches!(
            fm.format_item(&item).err().unwrap(),
            FormattingError::PlainFormattingError(_)
        ));

        let buf = Rc::new(RefCell::new(Vec::<u8>::new()));
        let mut fm = get_trivial_formatter("json", buf).unwrap();
        assert!(matches!(
            fm.format_item(&item).err().unwrap(),
            FormattingError::JsonFormatterError(_)
        ));
    }
}
