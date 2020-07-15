/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::any::type_name;
use std::error::Error;
use std::fmt::{self, Debug, Display, Formatter, Write};

use thiserror::Error;

/// Error Fault
///
/// If present, indicates that the fault originated Upstream (User), Downstream
/// (Dependency), or Internal to the system in question.  
#[derive(Copy, Clone, Hash, Debug)]
#[repr(u8)]
pub enum Fault {
    /// The error is the fault of the user, or some external part of the
    /// system that calls into this one. For instance, invalid command line
    /// arguments are a user error, even if the binary is being invoked by an
    /// automated system.
    User,

    /// The error is the fault of something internal to the system that
    /// produced the error. Generally speaking, this means a bug / programming
    /// error.
    Internal,

    /// The error is the fault of one of our dependencies, or any other system
    /// we call into. The developer should decide on a case-by-case basis
    /// whether to mark "user errors" reported by a dependency as Dependency,
    /// Internal, or User. If there's any doubt, mark it as Dependency, or
    /// leave the Fault untagged.
    Dependency,
}

impl Display for Fault {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        match self {
            &Fault::User => write!(f, "error is marked as user's fault"),
            &Fault::Internal => write!(f, "error is marked as an internal issue"),
            &Fault::Dependency => write!(f, "error is marked as a dependency issue"),
        }
    }
}

/// The name of the originating error type.
#[derive(Copy, Clone, Hash, Debug)]
#[repr(transparent)]
pub struct TypeName(pub &'static str);

impl Display for TypeName {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        write!(f, "error is marked with typename {:?}", self.0)
    }
}

impl TypeName {
    pub fn new<T>() -> Self {
        TypeName(type_name::<T>())
    }
}

#[derive(Copy, Clone, Debug)]
/// Common error metadata
pub struct CommonMetadata {
    pub fault: Option<Fault>,
    pub typename: Option<TypeName>,
}

impl Display for CommonMetadata {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        let mut prev = false;
        if let Some(typename) = self.typename {
            write!(f, "{}", typename)?;
            prev = true;
        }
        if let Some(fault) = self.fault {
            if prev {
                write!(f, ", ")?;
            }
            write!(f, "{}", fault)?;
            prev = true;
        }
        if !prev {
            write!(f, "no metadata")?;
        }
        Ok(())
    }
}

impl Default for CommonMetadata {
    fn default() -> Self {
        CommonMetadata {
            fault: None,
            typename: None,
        }
    }
}

impl CommonMetadata {
    pub fn new<T>() -> Self {
        CommonMetadata {
            fault: None,
            typename: Some(TypeName::new::<T>()),
        }
    }

    pub fn fault(mut self, fault: Fault) -> Self {
        self.fault = Some(fault);
        self
    }

    pub fn typename(mut self, typename: TypeName) -> Self {
        self.typename = Some(typename);
        self
    }

    /// Returns true if all CommonMetadata fields are filled, such that
    /// traversing the error tree will not provide any additional information.
    pub fn complete(&self) -> bool {
        self.fault.is_some() && self.typename.is_some()
    }

    pub fn merge(&mut self, other: &CommonMetadata) {
        self.fault = self.fault.or(other.fault);
        self.typename = self.typename.or(other.typename);
    }
}

pub trait AnyhowExt {
    fn mark_fault(self, fault: Fault) -> Self;
    fn mark_typename(self, typename: TypeName) -> Self;

    /// Traverse the error / context tree and assemble all CommonMetadata
    fn common_metadata(&self) -> CommonMetadata;
}

impl AnyhowExt for anyhow::Error {
    fn mark_fault(self, fault: Fault) -> Self {
        TaggedError::new(self, CommonMetadata::default().fault(fault)).wrapped()
    }

    fn mark_typename(self, typename: TypeName) -> Self {
        TaggedError::new(self, CommonMetadata::default().typename(typename)).wrapped()
    }

    fn common_metadata(&self) -> CommonMetadata {
        let mut metadata: CommonMetadata = Default::default();

        for cause in self.chain() {
            if let Some(e) = cause.downcast_ref::<TaggedError>() {
                metadata.merge(&e.metadata);
            }

            if metadata.complete() {
                break;
            }
        }
        metadata
    }
}

impl<T> AnyhowExt for anyhow::Result<T> {
    fn mark_fault(self, fault: Fault) -> Self {
        self.map_err(|e| e.mark_fault(fault))
    }

    fn mark_typename(self, typename: TypeName) -> Self {
        self.map_err(|e| e.mark_typename(typename))
    }

    fn common_metadata(&self) -> CommonMetadata {
        if let Some(errref) = self.as_ref().err() {
            errref.common_metadata()
        } else {
            Default::default()
        }
    }
}

/// A wapper type for errors which carries some common metadata.
///
/// If you already have an anyhow::Error, the AnyhowExt methods
/// are probably more ergonomic than directly using TaggedError.
#[derive(Debug, Error)]
#[error("{}", .metadata)]
pub struct TaggedError {
    pub source: anyhow::Error,
    pub metadata: CommonMetadata,
}

impl TaggedError {
    /// Construct a TaggedError with an error and metadata
    pub fn new(source: anyhow::Error, metadata: CommonMetadata) -> Self {
        TaggedError { source, metadata }
    }

    /// Wraps the TaggedError in an anyhow::Error
    pub fn wrapped(self) -> anyhow::Error {
        anyhow::Error::new(self)
    }
}

/// An error type with associated default metadata.
pub trait Tagged: Error + Send + Sync + Sized + 'static {
    /// Return the error, wrapped in an anyhow::Error, with metadata in the source chain.
    fn tagged(self) -> anyhow::Error {
        let metadata = self.metadata();
        TaggedError::new(self.into(), metadata).wrapped()
    }

    /// Override this to provide default metadata for an error type
    fn metadata(&self) -> CommonMetadata {
        CommonMetadata::new::<Self>()
    }
}

/// Controls how metadata is handled when formatting a FilteredAnyhow.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum PrintMode {
    /// Anyhow mode will show the nesting of metadata contexts. It is the most
    /// informative (you'll know if a tag was added by the original error's
    /// Tagged trait impl, or later by the AnyhowExt methods) but is messy and
    /// hard to read.
    Anyhow,

    /// NoTags mode completely suppresses metadata tags, filtering them out of
    /// the error chain, but otherwise using standard Anyhow error formatting.
    NoTags,

    /// SeparateTags mode is identical to NoTags, except that it collects the
    /// filtered errors, combining them into a single CommonMetadata (tags
    /// added later overriding those added earlier), and printing that metadata
    /// at the end of the formatted error message.
    SeparateTags,
}

/// A wrapper for anyhow which allows special handling of TaggedError metadata.
///
/// This should only be constructed in order to print an anyhow::Error that
/// might contain metadata, and is not meant to be wrapped in anyhow itself,
/// or otherwise passed around as an error wrapper type.
pub struct FilteredAnyhow<'a> {
    mode: PrintMode,
    pub err: &'a anyhow::Error,
}

impl<'a> FilteredAnyhow<'a> {
    pub fn new(err: &'a anyhow::Error) -> Self {
        FilteredAnyhow {
            err,
            mode: PrintMode::NoTags,
        }
    }

    pub fn with_mode(err: &'a anyhow::Error, mode: PrintMode) -> Self {
        FilteredAnyhow { mode, err }
    }

    pub fn no_tags(mut self) -> Self {
        self.mode = PrintMode::NoTags;
        self
    }

    pub fn separate_tags(mut self) -> Self {
        self.mode = PrintMode::SeparateTags;
        self
    }

    pub fn standard(mut self) -> Self {
        self.mode = PrintMode::Anyhow;
        self
    }
}

// Adapted from Anyhow's internal Display method
impl<'a> Display for FilteredAnyhow<'a> {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        use PrintMode::*;
        let mut filtered_chain = match self.mode {
            Anyhow => Box::new(self.err.chain())
                as Box<dyn Iterator<Item = &(dyn std::error::Error + 'static)>>,
            NoTags | SeparateTags => Box::new(self.err.chain().filter(|e| !e.is::<TaggedError>()))
                as Box<dyn Iterator<Item = &(dyn std::error::Error + 'static)>>,
        };

        // You can't construct an empty anyhow or an empty TaggedError, so
        // there will always be at least one entry in the chain regardless
        // of filtering mode.
        write!(f, "{}", filtered_chain.next().unwrap())?;

        if f.alternate() {
            for cause in filtered_chain {
                write!(f, ": {}", cause)?;
            }

            if self.mode == PrintMode::SeparateTags {
                write!(f, "\n\nTags: ")?;
                write!(f, "{}", self.err.common_metadata())?;
            }
        }

        Ok(())
    }
}

// Adapted from Anyhow's internal Debug method
impl<'a> Debug for FilteredAnyhow<'a> {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        if f.alternate() {
            return write!(f, "{:#?}", self.err);
        }

        use PrintMode::*;
        let mut filtered_chain = match self.mode {
            Anyhow => Box::new(self.err.chain())
                as Box<dyn Iterator<Item = &(dyn std::error::Error + 'static)>>,
            NoTags | SeparateTags => Box::new(self.err.chain().filter(|e| !e.is::<TaggedError>()))
                as Box<dyn Iterator<Item = &(dyn std::error::Error + 'static)>>,
        }
        .peekable();

        // You can't construct an empty anyhow or an empty TaggedError, so
        // there will always be at least one entry in the chain regardless
        // of filtering mode.
        write!(f, "{}", filtered_chain.next().unwrap())?;

        if let Some(cause) = filtered_chain.next() {
            write!(f, "\n\nCaused by:")?;
            let multiple = filtered_chain.peek().is_some();
            for (n, error) in std::iter::once(cause).chain(filtered_chain).enumerate() {
                writeln!(f)?;
                let mut indented = Indented {
                    inner: f,
                    number: if multiple { Some(n) } else { None },
                    started: false,
                };
                write!(indented, "{}", error)?;
            }
        }

        if self.mode == PrintMode::SeparateTags {
            write!(f, "\n\nTags: ")?;
            write!(f, "{}", self.err.common_metadata())?;
        }

        // No backtrace for now
        /*
        #[cfg(backtrace)]
        {
            use std::backtrace::BacktraceStatus;

            let backtrace = self.backtrace();
            if let BacktraceStatus::Captured = backtrace.status() {
                let mut backtrace = backtrace.to_string();
                write!(f, "\n\n")?;
                if backtrace.starts_with("stack backtrace:") {
                    // Capitalize to match "Caused by:"
                    backtrace.replace_range(0..1, "S");
                } else {
                    // "stack backtrace:" prefix was removed in
                    // https://github.com/rust-lang/backtrace-rs/pull/286
                    writeln!(f, "Stack backtrace:")?;
                }
                backtrace.truncate(backtrace.trim_end().len());
                write!(f, "{}", backtrace)?;
            }
        }
        */

        Ok(())
    }
}

// Taken directly from anyhow::Error, we're trying to match their formatting
struct Indented<'a, D> {
    inner: &'a mut D,
    number: Option<usize>,
    started: bool,
}

impl<T> Write for Indented<'_, T>
where
    T: Write,
{
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for (i, line) in s.split('\n').enumerate() {
            if !self.started {
                self.started = true;
                match self.number {
                    Some(number) => write!(self.inner, "{: >5}: ", number)?,
                    None => self.inner.write_str("    ")?,
                }
            } else if i > 0 {
                self.inner.write_char('\n')?;
                if self.number.is_some() {
                    self.inner.write_str("       ")?;
                } else {
                    self.inner.write_str("    ")?;
                }
            }

            self.inner.write_str(line)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_digit() {
        let input = "verify\nthis";
        let expected = "    2: verify\n       this";
        let mut output = String::new();

        Indented {
            inner: &mut output,
            number: Some(2),
            started: false,
        }
        .write_str(input)
        .unwrap();

        assert_eq!(expected, output);
    }

    #[test]
    fn two_digits() {
        let input = "verify\nthis";
        let expected = "   12: verify\n       this";
        let mut output = String::new();

        Indented {
            inner: &mut output,
            number: Some(12),
            started: false,
        }
        .write_str(input)
        .unwrap();

        assert_eq!(expected, output);
    }

    #[test]
    fn no_digits() {
        let input = "verify\nthis";
        let expected = "    verify\n    this";
        let mut output = String::new();

        Indented {
            inner: &mut output,
            number: None,
            started: false,
        }
        .write_str(input)
        .unwrap();

        assert_eq!(expected, output);
    }
}
