/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Result as IoResult;
use std::io::Write;

use crate::errors::FormatterNotFound;

pub struct FormatOptions {
    pub debug: bool,
    pub verbose: bool,
    pub quiet: bool,
}

pub trait Formattable {
    fn format_plain(&self, options: &FormatOptions, writer: &mut dyn Write) -> IoResult<()>;
}

pub trait ListFormatter {
    fn format_item(&mut self, item: &dyn Formattable) -> IoResult<()>;
}

pub struct PlainFormatter {
    writer: Box<dyn Write>,
    options: FormatOptions,
}

impl ListFormatter for PlainFormatter {
    fn format_item(&mut self, item: &dyn Formattable) -> IoResult<()> {
        item.format_plain(&self.options, self.writer.as_mut())
    }
}

pub fn get_formatter(
    _topic: &str,
    template: &str,
    options: FormatOptions,
    writer: Box<dyn Write>,
) -> Result<Box<dyn ListFormatter>, FormatterNotFound> {
    if template.is_empty() {
        return Ok(Box::new(PlainFormatter { writer, options }));
    }
    Err(FormatterNotFound(template.into()))
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;

    struct RequestTest<'a> {
        url: &'a str,
        result: u32,
    }

    impl<'a> Formattable for RequestTest<'a> {
        fn format_plain(&self, _options: &FormatOptions, writer: &mut dyn Write) -> IoResult<()> {
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

    #[test]
    fn test_formatter() {
        let buf = Rc::new(RefCell::new(Vec::<u8>::new()));
        let err = get_formatter(
            "",
            "{node|short}",
            FormatOptions {
                debug: false,
                verbose: false,
                quiet: false,
            },
            Box::new(Buffer {
                writer: buf.clone(),
            }),
        )
        .err()
        .unwrap();
        assert!(matches!(err, FormatterNotFound(_)));
        assert_eq!(
            err.to_string(),
            "unable to find formatter for template {node|short}"
        );

        let item = RequestTest {
            url: "foo://bar",
            result: 200,
        };
        let mut fm = get_formatter(
            "",
            "",
            FormatOptions {
                debug: false,
                verbose: false,
                quiet: false,
            },
            Box::new(Buffer {
                writer: buf.clone(),
            }),
        )
        .unwrap();
        fm.format_item(&item).unwrap();
        assert_eq!(
            String::from_utf8(buf.as_ref().borrow().clone()).unwrap(),
            "foo://bar: 200".to_string()
        );
    }
}
