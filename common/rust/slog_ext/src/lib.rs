/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use failure_ext::SlogKVErrorKey;
use slog::{self, Drain, OwnedKVList, Record, Serializer, KV};
use slog_term::Decorator;
use std::{fmt, io};

/// Drain that only prints the message and newline plus error if present, nothing more
pub struct SimpleFormatWithError<D: Decorator> {
    decorator: D,
}

impl<D: Decorator> SimpleFormatWithError<D> {
    pub fn new(decorator: D) -> Self {
        Self { decorator }
    }
}

impl<D: Decorator> Drain for SimpleFormatWithError<D> {
    type Ok = ();
    type Err = io::Error;

    fn log(
        &self,
        record: &Record,
        values: &OwnedKVList,
    ) -> ::std::result::Result<Self::Ok, Self::Err> {
        self.decorator.with_record(record, values, |decorator| {
            write!(decorator, "{}\n", record.msg())?;

            let mut serializer = ErrorSerializer {
                error: None,
                root_cause: None,
                backtrace: None,
                causes: Vec::new(),
            };
            record.kv().serialize(record, &mut serializer)?;
            values.serialize(record, &mut serializer)?;

            if let Some(error) = serializer.error {
                write!(decorator, "  Error:\n    {}\n", fix_indentation(error))?;

                if let Some(root_cause) = serializer.root_cause {
                    write!(
                        decorator,
                        "  Root cause:\n    {}\n",
                        fix_indentation(root_cause)
                    )?;
                }
                if let Some(backtrace) = serializer.backtrace {
                    write!(
                        decorator,
                        "  Backtrace:\n    {}\n",
                        fix_indentation(backtrace)
                    )?;
                }

                for cause in serializer.causes {
                    write!(decorator, "  Caused by:\n    {}\n", fix_indentation(cause))?;
                }
            }

            decorator.flush()?;
            Ok(())
        })
    }
}

struct ErrorSerializer {
    pub error: Option<String>,
    pub root_cause: Option<String>,
    pub backtrace: Option<String>,
    pub causes: Vec<String>,
}

impl Serializer for ErrorSerializer {
    fn emit_arguments(&mut self, key: slog::Key, val: &fmt::Arguments) -> slog::Result {
        if let Some(key) = SlogKVErrorKey::from_str(key) {
            use SlogKVErrorKey::*;
            match key {
                Error => self.error = non_empty_str_maybe(format!("{}", val)),
                RootCause => self.root_cause = non_empty_str_maybe(format!("{}", val)),
                Backtrace => self.backtrace = non_empty_str_maybe(format!("{}", val)),
                Cause => self.causes.push(format!("{}", val)),
            }
        }

        Ok(())
    }
}

fn non_empty_str_maybe(s: String) -> Option<String> {
    if s == "" {
        None
    } else {
        Some(s)
    }
}

fn fix_indentation(s: String) -> String {
    s.replace('\n', "\n    ")
}
