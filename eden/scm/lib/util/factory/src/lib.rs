/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Central place for registering how to construct `trait Foo` from inputs
//! (ex. urls, or paths, or configs).
//!
//! This can be useful to decouple implementation from abstraction.
//!
//! For example, the following code:
//!
//! ```ignore
//! // depending on impl1 or impl2.
//! fn construct_trait_foo(input: &str) -> anyhow::Result<Box<dyn Foo>> {
//!     if let Some(rest) = input.strip_prefix("impl1:") {
//!         Ok(Box::new(impl1::FooImpl::new(rest)?));
//!     } else if let Some(rest) = input.strip_prefix("impl2:") {
//!         Ok(Box::new(impl2::FooImpl::new(rest)?));
//!     } else {
//!         Err(...)
//!     }
//! }
//! ```
//!
//! Can be changed to:
//!
//! ```ignore
//! // without depending on impl1 or impl2.
//! fn construct_trait_foo(input: &str) -> anyhow::Result<Box<dyn Foo>> {
//!     factory::call_constructor(input)
//! }
//! ```
//!
//! If the `impl1` and `impl2` register their constructors:
//!
//! ```ignore
//! // Run this as part of startup.
//! fn register_impl1() {
//!     factory::register_constructor("impl1", |input: &str| -> anyhow::Result<Option<Box<dyn Foo>>> {
//!         match input.strip_prefix("impl1:") {
//!             None => Ok(None),
//!             Some(rest) => Ok(FooImpl::new(rest)?),
//!         }
//!     });
//! }
//! ```

use std::any;
use std::any::Any;
use std::any::TypeId;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::fmt;
use std::sync::OnceLock;
use std::sync::RwLock;

/// Register a constructor `func` to produce `Out` from `In`.
///
/// Can be called multiple times to register multiple constructors for a same
/// input/output type, but the `name` is unique per input/output type.
///
/// The return value of `func` could be:
/// - `Ok(None)`: continue try other registered functions.
/// - `Err(...)`: error out and stop trying other functions.
/// - `Ok(...)`: success and stop trying other functions.
///
/// When there are multiple constructors for the given input/output types, the
/// execution order is based on name.
pub fn register_constructor<In: 'static + ?Sized, Out: 'static>(
    name: &'static str,
    func: fn(&In) -> anyhow::Result<Option<Out>>,
) {
    tracing::debug!(
        in_type = any::type_name::<In>(),
        out_type = any::type_name::<Out>(),
        name = name,
        "registering constructor",
    );
    let dyn_func: BoxAny = Box::new(func) as BoxAny;
    let key = table_key::<In, Out>();
    let mut table = table().write().unwrap();
    table.entry(key).or_default().insert(name, dyn_func);
}

/// Call registered constructors to construct `Out`.
///
/// When there are multiple constructors for the given input/output types, the
/// execution order is based on name.
///
/// To test if any constructors are attempted (returning non-`None`),
/// use `is_any_constructor_attempted` on the error type.
pub fn call_constructor<In: 'static + ?Sized, Out: 'static>(input: &In) -> anyhow::Result<Out> {
    tracing::debug!(
        in_type = any::type_name::<In>(),
        out_type = any::type_name::<Out>(),
        "calling constructors",
    );
    let key = table_key::<In, Out>();
    let table = table().read().unwrap();
    let mut error_context = ErrorContext {
        from_type_name: any::type_name::<In>(),
        to_type_name: any::type_name::<Out>(),
        attempted_func_names: Vec::new(),
        error_func_name: None,
    };
    if let Some(registered) = table.get(&key) {
        for (name, dyn_func) in registered {
            tracing::trace!(" trying {}", name);
            let func: &fn(&In) -> anyhow::Result<Option<Out>> =
                dyn_func.downcast_ref().expect("typechecked by TypeId");
            match func(input) {
                Ok(None) => error_context.attempted_func_names.push(name),
                Ok(Some(v)) => return Ok(v),
                Err(e) => {
                    error_context.error_func_name = Some(name);
                    return Err(e.context(error_context));
                }
            }
        }
    }
    Err(error_context.into())
}

/// Returns `true` if the error is from a constructor, based on the `error`.
/// Returns `false` otherwise, or cannot decide.
pub fn is_error_from_constructor(error: &anyhow::Error) -> bool {
    if let Some(e) = error.downcast_ref::<ErrorContext>() {
        e.error_func_name.is_some()
    } else {
        false
    }
}

fn table_key<In: 'static + ?Sized, Out: 'static>() -> TypeId {
    TypeId::of::<fn(&In) -> Option<anyhow::Result<Out>>>()
}

type Table = RwLock<HashMap<TypeId, BTreeMap<&'static str, BoxAny>>>;
type BoxAny = Box<dyn Any + Send + Sync>;

fn table() -> &'static Table {
    static TABLE: OnceLock<Table> = OnceLock::new();
    TABLE.get_or_init(Default::default)
}

#[derive(Debug)]
struct ErrorContext {
    from_type_name: &'static str,
    to_type_name: &'static str,
    attempted_func_names: Vec<&'static str>,
    error_func_name: Option<&'static str>,
}

impl std::error::Error for ErrorContext {}

impl fmt::Display for ErrorContext {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "When constructing {} from {}",
            self.to_type_name, self.from_type_name,
        )?;
        if !self.attempted_func_names.is_empty() {
            write!(
                f,
                ", after being ignored by {:?}",
                &self.attempted_func_names
            )?;
        }
        if let Some(name) = self.error_func_name {
            write!(f, ", {:?} reported error", name)?;
        } else {
            write!(f, ", no registered functions were available")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty() {
        struct S;
        let err = call_constructor::<S, ()>(&S).unwrap_err();

        assert!(!is_error_from_constructor(&err));
        assert!(!is_error_from_constructor(&anyhow::anyhow!("error")));
    }

    #[test]
    fn test_single_constructor() {
        struct S(&'static str);
        register_constructor("parse int", |s: &S| -> anyhow::Result<Option<u32>> {
            Ok(Some(s.0.parse::<u32>()?))
        });
        assert_eq!(call_constructor::<_, u32>(&S("12")).unwrap(), 12);

        // Wrong type.
        let err = call_constructor::<_, i32>(&S("12")).unwrap_err();
        assert!(!is_error_from_constructor(&err));

        // Wrong input.
        let err = call_constructor::<_, u32>(&S("z")).unwrap_err();
        assert!(is_error_from_constructor(&err));
    }

    #[test]
    fn test_multiple_constructors() {
        struct S(&'static str);
        register_constructor("1 parse dec", |s: &S| -> anyhow::Result<Option<u32>> {
            if s.0.contains('x') {
                Ok(None)
            } else {
                Ok(Some(s.0.parse::<u32>()?))
            }
        });
        register_constructor("2 parse hex", |s: &S| -> anyhow::Result<Option<u32>> {
            match s.0.strip_prefix("0x") {
                None => Ok(None),
                Some(rest) => Ok(Some(u32::from_str_radix(rest, 16)?)),
            }
        });
        assert!(call_constructor::<_, u32>(&S("z")).is_err());
        assert!(call_constructor::<_, i32>(&S("12")).is_err());
        assert_eq!(call_constructor::<_, u32>(&S("12")).unwrap(), 12);
        assert_eq!(call_constructor::<_, u32>(&S("0x12")).unwrap(), 18);
    }

    #[test]
    fn test_unsized() {
        #[derive(Debug)]
        struct O(usize);
        register_constructor("unsized", |s: &str| -> anyhow::Result<Option<O>> {
            Ok(Some(O(s.len())))
        });
        assert_eq!(call_constructor::<str, O>("foo").unwrap().0, 3);
    }
}
