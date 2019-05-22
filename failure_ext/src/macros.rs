// Copyright 2004-present Facebook. All Rights Reserved.

/// Exits a function early with an `Error`.
///
/// The `bail!` macro provides an easy way to exit a function. `bail_err!(X)` is
/// equivalent to writing:
///
/// ```rust,ignore
/// return Err(From::from(X));
/// ```
#[macro_export]
macro_rules! bail_err {
    ($e:expr) => {
        return Err(From::from($e));
    };
}

/// Exits a function early with an `Error`.
///
/// The `bail!` macro provides an easy way to exit a function. `bail_msg!(X)` is
/// equivalent to writing:
///
/// ```rust,ignore
/// return Err(format_err!(X));
/// ```
#[macro_export]
macro_rules! bail_msg {
    ($e:expr) => {
        return Err($crate::err_msg($e));
    };
    ($fmt:expr, $($arg:tt)+) => {
        return Err($crate::err_msg(format!($fmt, $($arg)+)));
    };
}

/// Exits a function early with an `Error` if the condition is not satisfied.
///
/// Similar to `assert!`, `ensure!` takes a condition and exits the function
/// if the condition fails. Unlike `assert!`, `ensure!` returns an `Error`,
/// it does not panic.
#[macro_export]
macro_rules! ensure_err {
    ($cond:expr, $e:expr) => {
        if !($cond) {
            $crate::bail_err!($e);
        }
    };
}

#[macro_export]
macro_rules! ensure_msg {
    ($cond:expr, $e:expr) => {
        if !($cond) {
            $crate::bail_msg!($e);
        }
    };
    ($cond:expr, $fmt:expr, $($arg:tt)+) => {
        if !($cond) {
            $crate::bail_msg!($fmt, $($arg)+);
        }
    };
}

/// Constructs an `Error` using the standard string interpolation syntax.
///
/// ```rust
/// #[macro_use] extern crate failure;
///
/// fn main() {
///     let code = 101;
///     let err = format_err!("Error code: {}", code);
/// }
/// ```
#[macro_export]
macro_rules! format_err {
    ($($arg:tt)*) => { $crate::err_msg(format!($($arg)*)) }
}

/// Downcast matching
/// Usage:
/// ```
/// let res = err_downcast_ref! {
///    err,
///    ty: Type => { use ty as &Type }
///    yours: YourType => { use yours as &YourType }
/// };
/// ```
///
/// Where `err` is a `&failure::Error`.
/// When one of the type arms match, then it returns Some(value from expr), otherwise None.
/// It matches against `type`, but also `Chain<type>` and `Context<Type>`.
#[macro_export]
macro_rules! err_downcast_ref {
    // Base case - all patterns consumed
    ( $err:expr ) => {
        { let _ = $err; None }
    };
    // Eliminate trailing comma
    ( $err:expr, $($v:ident : $ty:ty => $action:expr),* , ) => {
        err_downcast_ref!($err, $($v : $ty => $action),*)
    };
    // Default case - match one type pattern, and recur with the rest of the list.
    // The rest of the list consumes the , separating it from the first pattern and
    // is itself comma-separated, with no trailing comma
    ( $err:expr, $v:ident : $ty:ty => $action:expr $(, $rv:ident : $rty:ty => $raction:expr)* ) => {{
        match $err.downcast_ref::<$ty>() {
            Some($v) => Some($action),
            None => match $err.downcast_ref::<$crate::failure::Context<$ty>>() {
                Some(c) => { let $v = c.get_context(); Some($action) },
                None => match $err.downcast_ref::<$crate::chain::Chain<$ty>>() {
                    Some(c) => { let $v = c.as_err(); Some($action) },
                    None => err_downcast_ref!($err $(, $rv : $rty => $raction)*),
                }
            }
        }
    }};
}

/// Downcast matching
/// Usage:
/// ```
/// let res = err_downcast! {
///    err,
///    ty: Type => { use ty as Type }
///    yours: YourType => { use yours as YourType }
/// };
/// ```
///
/// Where `err` is a `failure::Error`.
/// When one of the type arms match, then it returns Ok(value from expr), otherwise Err(err).
/// It matches against `type`, but also `Chain<type>`. (`Context` can't be supported as it
/// doesn't have an `into_context()` method).
#[macro_export]
macro_rules! err_downcast {
    // Base case - all patterns consumed
    ( $err:expr ) => {
        Err($err)
    };
    // Eliminate trailing comma
    ( $err:expr, $($v:ident : $ty:ty => $action:expr),* , ) => {
        err_downcast!($err, $($v : $ty => $action),*)
    };
    // Default case - match one type pattern, and recur with the rest of the list.
    // The rest of the list consumes the , separating it from the first pattern and
    // is itself comma-separated, with no trailing comma
    ( $err:expr, $v:ident : $ty:ty => $action:expr $(, $rv:ident : $rty:ty => $raction:expr)* ) => {{
        match $err.downcast::<$ty>() {
            Ok($v) => Ok($action),
            Err(other) => match other.downcast::<$crate::chain::Chain<$ty>>() {
                Ok(c) => { let $v = c.into_err(); Ok($action) },
                Err(other) => err_downcast!(other $(, $rv : $rty => $raction)*),
            }
        }
    }};
}

#[cfg(test)]
mod test {
    use crate::prelude::*;

    #[derive(Fail, Debug)]
    #[fail(display = "Foo badness")]
    struct Foo;
    #[derive(Fail, Debug)]
    #[fail(display = "Bar badness")]
    struct Bar;
    #[derive(Fail, Debug)]
    #[fail(display = "Blat badness")]
    struct Blat;
    #[derive(Fail, Debug)]
    #[fail(display = "Outer badness")]
    struct Outer;

    #[test]
    fn downcast_ref_syntax() {
        let blat = Error::from(Blat);

        // Single, tailing ,
        let _ = err_downcast_ref! {
            blat,
            v: Foo => v.to_string(),
        };

        // Single, no tailing ,
        let _ = err_downcast_ref! {
            blat,
            v: Foo => v.to_string()
        };

        // Multi, tailing ,
        let _ = err_downcast_ref! {
            blat,
            v: Foo => v.to_string(),
            v: Blat => v.to_string(),
        };

        // Multi, no tailing ,
        let _ = err_downcast_ref! {
            blat,
            v: Foo => v.to_string(),
            v: Blat => v.to_string()
        };
    }

    #[test]
    fn downcast_ref_basic() {
        let blat = Error::from(Blat);

        let msg = err_downcast_ref! {
            blat,
            foo: Foo => foo.to_string(),
            bar: Bar => bar.to_string(),
            blat: Blat => blat.to_string(),
            outer: Outer => outer.to_string(),
        };

        assert_eq!(msg.unwrap(), "Blat badness".to_string());
    }

    #[test]
    fn downcast_ref_context() {
        let foo = Error::from(Foo);
        let outer = Error::from(foo.context(Outer));

        let msg = err_downcast_ref! {
            outer,
            foo: Foo => foo.to_string(),
            bar: Bar => bar.to_string(),
            blat: Blat => blat.to_string(),
            outer: Outer => outer.to_string(),
        };

        assert_eq!(msg.unwrap(), "Outer badness".to_string());
    }

    #[test]
    fn downcast_ref_chain() {
        let foo = Error::from(Foo);
        let outer = Error::from(foo.chain_err(Outer));

        let msg = err_downcast_ref! {
            outer,
            v: Foo => { let _: &Foo = v; v.to_string() },
            v: Bar => { let _: &Bar = v; v.to_string() },
            v: Blat => { let _: &Blat = v; v.to_string() },
            v: Outer => { let _: &Outer = v; v.to_string() },
        };

        assert_eq!(msg.unwrap(), "Outer badness".to_string());
    }

    #[test]
    fn downcast_ref_miss() {
        let blat = Error::from(Blat);

        let msg = err_downcast_ref! {
            blat,
            v: Foo => { let _: &Foo = v; v.to_string() },
            v: Bar => { let _: &Bar = v; v.to_string() },
        };

        assert!(msg.is_none());
        assert!(blat.downcast_ref::<Blat>().is_some());
    }

    #[test]
    fn downcast_syntax() {
        // Single, tailing ,
        let blat = Error::from(Blat);
        let _ = err_downcast! {
            blat,
            v: Foo => v.to_string(),
        };

        // Single, no tailing ,
        let blat = Error::from(Blat);
        let _ = err_downcast! {
            blat,
            v: Foo => v.to_string()
        };

        // Multi, tailing ,
        let blat = Error::from(Blat);
        let _ = err_downcast! {
            blat,
            v: Foo => v.to_string(),
            v: Blat => v.to_string(),
        };

        // Multi, no tailing ,
        let blat = Error::from(Blat);
        let _ = err_downcast! {
            blat,
            v: Foo => v.to_string(),
            v: Blat => v.to_string()
        };
    }

    #[test]
    fn downcast_basic() {
        let blat = Error::from(Blat);

        let msg = err_downcast! {
            blat,
            foo: Foo => foo.to_string(),
            bar: Bar => bar.to_string(),
            blat: Blat => blat.to_string(),
            outer: Outer => outer.to_string(),
        };

        assert_eq!(msg.unwrap(), "Blat badness".to_string());
    }

    #[test]
    fn downcast_chain() {
        let foo = Error::from(Foo);
        let outer = Error::from(foo.chain_err(Outer));

        let msg = err_downcast! {
            outer,
            v: Foo => { let _: Foo = v; v.to_string() },
            v: Bar => { let _: Bar = v; v.to_string() },
            v: Blat => { let _: Blat = v; v.to_string() },
            v: Outer => { let _: Outer = v; v.to_string() },
        };

        assert_eq!(msg.unwrap(), "Outer badness".to_string());
    }

    #[test]
    fn downcast_miss() {
        let blat = Error::from(Blat);

        let msg = err_downcast! {
            blat,
            foo: Foo => foo.to_string(),
            bar: Bar => bar.to_string(),
            outer: Outer => outer.to_string(),
        };

        assert!(msg.is_err());
        assert!(msg.unwrap_err().downcast::<Blat>().is_ok());
    }
}
