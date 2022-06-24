/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[macro_export]
macro_rules! abort_if {
    ( $cond:expr, $($arg:tt)+ ) => {
        if $cond {
            abort!($($arg)*);
        }
    };
}

#[macro_export]
macro_rules! abort {
    ( $msg:expr ) => {
        return Err(clidispatch::errors::Abort($msg.into()).into());
    };
    ( $($arg:tt)+ ) => {
        return Err(clidispatch::errors::Abort(format!($($arg)*).into()).into());
    };
}

#[cfg(test)]
mod test {
    use crate as clidispatch;

    fn abort_if_simple(should_abort: bool) -> anyhow::Result<()> {
        abort_if!(should_abort, "error!");
        Ok(())
    }

    fn abort_if_format(should_abort: bool) -> anyhow::Result<()> {
        abort_if!(should_abort, "error: {}", "banana");
        Ok(())
    }

    #[test]
    fn test_abort_if() {
        assert!(abort_if_simple(false).is_ok());
        assert_eq!(format!("{}", abort_if_simple(true).unwrap_err()), "error!");

        assert!(abort_if_format(false).is_ok());
        assert_eq!(
            format!("{}", abort_if_format(true).unwrap_err()),
            "error: banana",
        );
    }

    fn abort_simple() -> anyhow::Result<()> {
        abort!("error!");
    }

    fn abort_format() -> anyhow::Result<()> {
        abort!("error: {}", "banana");
    }

    #[test]
    fn test_abort() {
        assert_eq!(format!("{}", abort_simple().unwrap_err()), "error!");
        assert_eq!(format!("{}", abort_format().unwrap_err()), "error: banana",);
    }
}
