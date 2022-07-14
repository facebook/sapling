/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::bail;
use anyhow::Error;
use failure_ext::SlogKVErrorKey;
use slog::Drain;
use slog::Never;
use slog::OwnedKVList;
use slog::Record;
use slog::Serializer;
use slog::KV;
use slog_term::Decorator;
use std::collections::HashSet;
use std::fmt;
use std::io;
use std::str::FromStr;

// Allow us to switch drain types without runtime check
enum EitherDrain<L, R> {
    Left(L),
    Right(R),
}

impl<O, E, L: Drain<Ok = O, Err = E>, R: Drain<Ok = O, Err = E>> Drain for EitherDrain<L, R> {
    type Ok = O;
    type Err = E;

    fn log(&self, record: &Record, values: &OwnedKVList) -> Result<Self::Ok, Self::Err> {
        match self {
            EitherDrain::Left(d) => d.log(record, values),
            EitherDrain::Right(d) => d.log(record, values),
        }
    }
}

// Filter in or out log messages based on the slog::Record::tag() (which in turn is mapped from Log::Record::target())
struct TagFilterDrain<D> {
    inner: D,
    include_tags: HashSet<String>,
    exclude_tags: HashSet<String>,
    pass_untagged: bool,
}

impl<D> TagFilterDrain<D> {
    fn should_log(&self, tag: &str) -> bool {
        if tag.is_empty() {
            return self.pass_untagged;
        }
        if self.exclude_tags.contains(tag) {
            return false;
        }
        if self.include_tags.is_empty() {
            return true;
        }
        self.include_tags.contains(tag)
    }
}

impl<D: Drain<Ok = (), Err = Never>> Drain for TagFilterDrain<D> {
    type Ok = ();
    type Err = Never;

    fn log(&self, record: &Record, values: &OwnedKVList) -> Result<Self::Ok, Self::Err> {
        if self.should_log(record.tag()) {
            self.inner.log(record, values)
        } else {
            Ok(())
        }
    }
}

pub fn make_tag_filter_drain<'a, D>(
    inner: D,
    include_tags: HashSet<String>,
    exclude_tags: HashSet<String>,
    pass_untagged: bool,
) -> Result<impl Drain<Ok = (), Err = Never> + 'a, Error>
where
    D: Drain<Ok = (), Err = Never> + 'a,
{
    if include_tags.is_empty() && exclude_tags.is_empty() {
        Ok(EitherDrain::Left(inner))
    } else {
        let intersection = include_tags
            .intersection(&exclude_tags)
            .collect::<HashSet<_>>();
        if !intersection.is_empty() {
            bail!(
                "Following tags are in both the include and exclude sets: {:?}",
                intersection
            );
        } else {
            Ok(EitherDrain::Right(TagFilterDrain {
                inner,
                include_tags,
                exclude_tags,
                pass_untagged,
            }))
        }
    }
}

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

    fn log(&self, record: &Record, values: &OwnedKVList) -> Result<Self::Ok, Self::Err> {
        self.decorator.with_record(record, values, |decorator| {
            write!(decorator, "{}\n", record.msg())?;

            let mut serializer = ErrorSerializer {
                error: None,
                root_cause: None,
                backtrace: None,
                causes: Vec::new(),
                error_debug: None,
            };
            record.kv().serialize(record, &mut serializer)?;
            values.serialize(record, &mut serializer)?;

            if let Some(error) = serializer.error {
                write!(decorator, "  Error:\n    {}\n", fix_indentation(error))?;

                if let Some(root_cause) = serializer.root_cause {
                    write!(decorator, "\n")?;
                    write!(
                        decorator,
                        "  Root cause:\n    {}\n",
                        fix_indentation(root_cause)
                    )?;
                }

                if let Some(backtrace) = serializer.backtrace {
                    write!(decorator, "\n")?;
                    write!(
                        decorator,
                        "  Backtrace:\n    {}\n",
                        fix_indentation(backtrace)
                    )?;
                }

                for (i, cause) in serializer.causes.into_iter().enumerate() {
                    if i == 0 {
                        write!(decorator, "\n")?;
                    }
                    write!(decorator, "  Caused by:\n    {}\n", fix_indentation(cause))?;
                }

                if let Some(error_debug) = serializer.error_debug {
                    write!(decorator, "\n")?;
                    write!(
                        decorator,
                        "  Debug context:\n    {}\n",
                        fix_indentation(error_debug)
                    )?;
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
    pub error_debug: Option<String>,
}

impl Serializer for ErrorSerializer {
    fn emit_arguments(&mut self, key: slog::Key, val: &fmt::Arguments) -> slog::Result {
        if let Ok(key) = SlogKVErrorKey::from_str(key) {
            use SlogKVErrorKey::*;
            match key {
                Error => self.error = non_empty_str_maybe(format!("{}", val)),
                RootCause => self.root_cause = non_empty_str_maybe(format!("{}", val)),
                Backtrace => self.backtrace = non_empty_str_maybe(format!("{}", val)),
                Cause => self.causes.push(format!("{}", val)),
                ErrorDebug => self.error_debug = non_empty_str_maybe(format!("{}", val)),
            }
        }

        Ok(())
    }
}

fn non_empty_str_maybe(s: String) -> Option<String> {
    if s.is_empty() { None } else { Some(s) }
}

fn fix_indentation(s: String) -> String {
    s.replace('\n', "\n    ")
}
