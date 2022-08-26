/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;

use crate::errors::FormatterNotFound;
use crate::errors::FormattingError;

pub type FormatResult<T> = std::result::Result<T, FormattingError>;

pub struct FormatOptions {
    pub debug: bool,
    pub verbose: bool,
    pub quiet: bool,
}

pub trait Formattable {
    fn format_plain(
        &self,
        options: &FormatOptions,
        writer: &mut dyn Write,
    ) -> Result<(), anyhow::Error>;
}

pub trait ListFormatter {
    fn format_item(&mut self, item: &dyn Formattable) -> FormatResult<()>;
}

pub struct PlainFormatter {
    writer: Box<dyn Write>,
    options: FormatOptions,
}

impl ListFormatter for PlainFormatter {
    fn format_item(&mut self, item: &dyn Formattable) -> FormatResult<()> {
        item.format_plain(&self.options, self.writer.as_mut())
            .map_err(|err| match err.downcast::<std::io::Error>() {
                Ok(io_err) => FormattingError::WriterError(io_err),
                Err(err) => FormattingError::PlainFormattingError(err),
            })
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
    use std::io::Result as IoResult;
    use std::rc::Rc;

    use anyhow::bail;

    use super::*;

    struct RequestTest<'a> {
        url: &'a str,
        result: u32,
    }

    impl<'a> Formattable for RequestTest<'a> {
        fn format_plain(
            &self,
            _options: &FormatOptions,
            writer: &mut dyn Write,
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
            _writer: &mut dyn Write,
        ) -> Result<(), anyhow::Error> {
            bail!("Nope")
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

    #[test]
    fn test_errors() {
        let buf: [u8; 0] = [0; 0];
        let mut fm = get_formatter(
            "",
            "",
            FormatOptions {
                debug: false,
                verbose: false,
                quiet: false,
            },
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
    }
}
