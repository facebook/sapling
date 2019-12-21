/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cpython::*;
use hgtime::HgTime;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "hgtime"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add(py, "parse", py_fn!(py, parse(content: &str)))?;
    m.add(py, "parserange", py_fn!(py, parserange(content: &str)))?;
    m.add(
        py,
        "setnowfortesting",
        py_fn!(py, setnowfortesting(content: &str)),
    )?;

    // The Rust library indirectly uses libc for timezone handling.
    // Expose tzset() from libc so chg server can update timezone
    // environments when $TZ gets changed.
    m.add(py, "tzset", py_fn!(py, pytzset()))?;

    // Initialize. Useful on Windows.
    tzset();
    Ok(m)
}

fn parse(_py: Python, content: &str) -> PyResult<Option<(i64, i32)>> {
    Ok(HgTime::parse(content).map(|t| (t.unixtime, t.offset)))
}

fn parserange(_py: Python, content: &str) -> PyResult<Option<((i64, i32), (i64, i32))>> {
    Ok(HgTime::parse_range(content).map(|r| {
        (
            (r.start.unixtime, r.start.offset),
            (r.end.unixtime, r.end.offset),
        )
    }))
}

fn setnowfortesting(py: Python, content: &str) -> PyResult<PyObject> {
    if let Some(time) = HgTime::parse(content) {
        time.set_as_now_for_testing();
    }
    Ok(py.None())
}

fn pytzset(_: Python) -> PyResult<Option<i32>> {
    Ok(tzset())
}

/// Initialize timezone setting by reading the `TZ` environment variable.
/// Return the timezone offset in seconds (without daylight savings).
///
/// On Windows, the daylight saving handling is currently broken if `TZ`
/// is set. But settings from the Windows Control Panel (without `TZ`)
/// should work as expected.
fn tzset() -> Option<i32> {
    #[cfg(unix)]
    {
        use std::os::raw::c_long;
        extern "C" {
            // See https://www.gnu.org/software/libc/manual/html_node/Time-Zone-Functions.html#Time-Zone-Functions
            fn tzset();
            #[no_mangle]
            static timezone: c_long;
        }
        unsafe { tzset() };
        return Some(unsafe { timezone } as i32);
    }
    #[cfg(windows)]
    {
        use std::os::raw::{c_int, c_long};
        extern "C" {
            // See https://docs.microsoft.com/en-us/cpp/c-runtime-library/daylight-dstbias-timezone-and-tzname?view=vs-2019
            fn _tzset();
            fn _get_timezone(seconds: *mut c_long) -> c_int;
        }
        unsafe { _tzset() };
        let mut timezone = 0;
        unsafe { _get_timezone(&mut timezone) };
        // The time-0.1.42 crate calls FileTimeToSystemTime on Windows which
        // does not respect $TZ. Ideally it should call functions from MSVC
        // runtime instead. To reduce issues, use `set_default_offset` to
        // tell `hgtime` to use a fixed timezone offset. Note: it does not
        // really work for complex timezones with daylight savings. But it
        // is good enough for TZ=GMT cases (which is used by tests).
        if std::env::var_os("TZ").is_some() {
            hgtime::set_default_offset(timezone);
        }
        return Some(timezone as i32);
    }
    #[allow(unreachable_code)]
    None
}
