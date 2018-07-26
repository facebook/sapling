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
            bail_err!($e);
        }
    };
}

#[macro_export]
macro_rules! ensure_msg {
    ($cond:expr, $e:expr) => {
        if !($cond) {
            bail_msg!($e);
        }
    };
    ($cond:expr, $fmt:expr, $($arg:tt)+) => {
        if !($cond) {
            bail_msg!($fmt, $($arg)+);
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
