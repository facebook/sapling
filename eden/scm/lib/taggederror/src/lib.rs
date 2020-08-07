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
/// If present, indicates that the fault originated Upstream (Request), Downstream
/// (Dependency), or Internal to the system in question.  
#[derive(Copy, Clone, Hash, Debug)]
#[repr(u8)]
pub enum Fault {
    /// The error is the fault of the request, or some external part of the
    /// system that calls into this one. For instance, invalid command line
    /// arguments are a request error, even if the binary is being invoked by
    /// an automated system.
    Request,

    /// The error is the fault of something internal to the system that
    /// produced the error. Generally speaking, this means a bug / programming
    /// error.
    Internal,

    /// The error is the fault of one of our dependencies, or any other system
    /// we call into. The developer should decide on a case-by-case basis
    /// whether to mark "request errors" reported by a dependency as Dependency,
    /// Internal, or Request. If there's any doubt, mark it as Dependency, or
    /// leave the Fault untagged.
    Dependency,
}

impl Display for Fault {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        match *self {
            Fault::Request => write!(f, "error is request issue"),
            Fault::Internal => write!(f, "error is internal issue"),
            Fault::Dependency => write!(f, "error is dependency issue"),
        }
    }
}

/// The name of the originating error type.
#[derive(Copy, Clone, Hash, Debug)]
#[repr(transparent)]
pub struct TypeName(pub &'static str);

impl Display for TypeName {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        write!(f, "error has type name {:?}", self.0)
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
    pub type_name: Option<TypeName>,
}

impl Display for CommonMetadata {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        let mut prev = false;
        if let Some(type_name) = self.type_name {
            write!(f, "{}", type_name)?;
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
            type_name: None,
        }
    }
}

impl CommonMetadata {
    pub fn new<T>() -> Self {
        CommonMetadata {
            fault: None,
            type_name: Some(TypeName::new::<T>()),
        }
    }

    pub fn with_fault(mut self, fault: Fault) -> Self {
        self.fault = Some(fault);
        self
    }

    pub fn with_type_name(mut self, type_name: TypeName) -> Self {
        self.type_name = Some(type_name);
        self
    }

    /// Returns true if all CommonMetadata fields are filled, such that
    /// traversing the error tree will not provide any additional information.
    pub fn complete(&self) -> bool {
        self.fault.is_some() && self.type_name.is_some()
    }

    pub fn empty(&self) -> bool {
        self.fault.is_none() && self.type_name.is_none()
    }

    pub fn merge(&mut self, other: &CommonMetadata) {
        self.fault = self.fault.or(other.fault);
        self.type_name = self.type_name.or(other.type_name);
    }
}

pub trait AnyhowExt {
    fn with_fault(self, fault: Fault) -> Self;
    fn with_type_name(self, type_name: TypeName) -> Self;
    fn with_metadata(self, metadata: CommonMetadata) -> Self;

    /// Traverse the error / context tree and assemble all CommonMetadata
    fn common_metadata(&self) -> CommonMetadata;
}

impl AnyhowExt for anyhow::Error {
    fn with_fault(self, fault: Fault) -> Self {
        TaggedError::new(self, CommonMetadata::default().with_fault(fault)).wrapped()
    }

    fn with_type_name(self, typename: TypeName) -> Self {
        TaggedError::new(self, CommonMetadata::default().with_type_name(typename)).wrapped()
    }

    fn with_metadata(self, metadata: CommonMetadata) -> Self {
        TaggedError::new(self, metadata).wrapped()
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
    fn with_fault(self, fault: Fault) -> Self {
        self.map_err(|e| e.with_fault(fault))
    }

    fn with_type_name(self, typename: TypeName) -> Self {
        self.map_err(|e| e.with_type_name(typename))
    }

    fn with_metadata(self, metadata: CommonMetadata) -> Self {
        self.map_err(|e| e.with_metadata(metadata))
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

#[derive(Debug, Error)]
#[error("intentional error for debugging with message '{0}'")]
pub struct IntentionalError(String);

impl Tagged for IntentionalError {
    fn metadata(&self) -> CommonMetadata {
        // CommonMetadata::new::<Self>() attaches typename
        CommonMetadata::new::<Self>()
    }
}

pub fn intentional_error(tagged: bool) -> anyhow::Result<u8> {
    if tagged {
        // Metadata explicitly attached with .tagged()
        return Err(IntentionalError(String::from("intentional_error")).tagged());
    } else {
        // Metadata is automatically associated by taggederror_util::AnyhowEdenExt
        bail!(IntentionalError(String::from("intentional_error")))
    }
}

pub fn intentional_bail() -> anyhow::Result<u8> {
    bail!(
        fault = Fault::Request,
        TypeName("taggederror::FakeTypeNameForTesting"),
        "intentional bail with {}",
        "format params"
    )
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

/// A drop-in replacement for the anyhow::bail macro, which allows applying error metadata.
///
/// Supports all three styles of `bail!` calls supported by anyhow
///
/// String literal: `bail!("literal error message")`
/// Display Expression: `bail!(my_expr_impls_display)`
/// Format Expression: `bail!("failure in {} system", "logging")`
///
/// You can provide metadata for these errors by prepending it to the
/// `bail` argument list. Metadata can be provided with in two styles,
/// `key = value` and "literal" syntax. These forms can be mixed as desired in
/// the same call.
///
/// Literal style: `bail!(Fault::Request, TypeName("fakemod::FakeTypeName"), "standard bail args")`
/// Key-value style: `bail!(fault = my_fault(), type_name = TypeName(my_static_str()), "bail format {}", "args")`
/// Mixed: `bail!(Fault::Request, type_name = my_typename(), "bail message")`
#[macro_export]
macro_rules! bail {
    // Bail variations with metadata
    (@withmeta $meta:expr, $msg:literal $(,)?) => {
        return std::result::Result::Err(anyhow::anyhow!($msg).with_metadata($meta));
    };
    (@withmeta $meta:expr, $err:expr $(,)?) => {
        return std::result::Result::Err(anyhow::anyhow!($err).with_metadata($meta));
    };
    (@withmeta $meta:expr, $fmt:expr, $($arg:tt)*) => {
        return std::result::Result::Err(anyhow::anyhow!($fmt, $($arg)*).with_metadata($meta));
    };

    // Metadata munching
    // Concise syntax for literal metadata
    (@metadata $meta:expr, Fault::$fault:ident, $($tail:tt)+) => {
        bail!(@metadata CommonMetadata::with_fault($meta, Fault::$fault), $($tail)+)
    };
    (@metadata $meta:expr, TypeName($type_name:expr), $($tail:tt)+) => {
        bail!(@metadata CommonMetadata::with_type_name($meta, TypeName($type_name)), $($tail)+)
    };
    // More verbose key=value syntax for metadata expressions
    (@metadata $meta:expr, fault=$fault:expr, $($tail:tt)+) => {
        bail!(@metadata CommonMetadata::with_fault($meta, $fault), $($tail)+)
    };
    (@metadata $meta:expr, type_name=$type_name:expr, $($tail:tt)+) => {
        bail!(@metadata CommonMetadata::with_type_name($meta, $type_name), $($tail)+)
    };

    // Metadata base case, trailing bail args
    (@metadata $meta:expr, $($args:tt)+) => {
        bail!(@withmeta $meta, $($args)+)
    };

    // Metadata entry points
    (Fault::$fault:ident, $($tail:tt)+) => {
        bail!(@metadata CommonMetadata::default().with_fault(Fault::$fault), $($tail)+)
    };
    (TypeName($type_name:expr), $($tail:tt)+) => {
        bail!(@metadata CommonMetadata::default().with_type_name(TypeName($type_name)), $($tail)+)
    };
    (fault=$fault:expr, $($tail:tt)+) => {
        bail!(@metadata CommonMetadata::default().with_fault($fault), $($tail)+)
    };
    (type_name=$type_name:expr, $($tail:tt)+) => {
        bail!(@metadata CommonMetadata::default().with_type_name($type_name), $($tail)+)
    };

    // Bail variations without metadata
    ($msg:literal $(,)?) => {
        return std::result::Result::Err(anyhow::anyhow!($msg));
    };
    ($err:expr $(,)?) => {
        return std::result::Result::Err(anyhow::anyhow!($err));
    };
    ($fmt:expr, $($arg:tt)*) => {
        return std::result::Result::Err(anyhow::anyhow!($fmt, $($arg)*));
    };
}

/// A wrapper for anyhow which allows special handling of TaggedError metadata.
///
/// This should only be constructed in order to print an anyhow::Error that
/// might contain metadata, and is not meant to be wrapped in anyhow itself,
/// or otherwise passed around as an error wrapper type.
pub struct FilteredAnyhow<'a> {
    pub err: &'a anyhow::Error,
    mode: PrintMode,
    metadata_func: fn(&'a anyhow::Error) -> CommonMetadata,
}

impl<'a> FilteredAnyhow<'a> {
    pub fn new(err: &'a anyhow::Error) -> Self {
        FilteredAnyhow {
            err,
            mode: PrintMode::NoTags,
            metadata_func: |e| e.common_metadata(),
        }
    }

    pub fn with_metadata_func(mut self, func: fn(&'a anyhow::Error) -> CommonMetadata) -> Self {
        self.metadata_func = func;
        self
    }

    pub fn with_mode(mut self, mode: PrintMode) -> Self {
        self.mode = mode;
        self
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
                write!(f, "\n\nerror tags: ")?;
                write!(f, "{}", (self.metadata_func)(self.err))?;
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
            write!(f, "\n\nerror tags: ")?;
            write!(f, "{}", (self.metadata_func)(self.err))?;
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
