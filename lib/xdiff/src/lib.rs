// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Functions to find the difference between two texts.
//! Under the hood it's using the xdiff library that's also used by git and hg.

use std::ops::Range;
use std::os::raw::{c_char, c_int, c_void};
use xdiff_sys as ffi;

/// An individual difference between two texts. Consists of two
/// line ranges that specify which parts of the texts differ.
///
/// If any of the ranges is empty it's still significant because it
/// specifies the location in the text where the other range applies.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Hunk {
    /// Line range from new text to insert
    pub add: Range<usize>,
    /// Line from the old text to delete
    pub remove: Range<usize>,
}

/// Computes the hunks of differerences between two texts.
///
/// Takes two byte-slices as arguments.
/// Returns a list of hunks that indicate differences between the old and new text.
///
/// # Example
/// ```
/// use xdiff::{diff_hunks, Hunk};
/// let a = "a\n b\n c\n d\n";
/// let b = "a\n c\n d\n e\n";
/// let c = "a\n b\n c\n d\n e\n";
/// assert_eq!(
///     diff_hunks(a, b),
///     [
///         Hunk {
///             add: 1..1,
///             remove: 1..2,
///         },
///         Hunk {
///             add: 3..4,
///             remove: 4..4,
///         }
///     ]
/// );
/// assert_eq!(
///     diff_hunks(a, c),
///     [Hunk {
///         add: 4..5,
///         remove: 4..4
///     }]
/// );
/// let x = diff_hunks(b, c);
/// assert_eq!(x[0].add.end, 2);
/// ```
pub fn diff_hunks<T>(old_text: T, new_text: T) -> Vec<Hunk>
where
    T: AsRef<[u8]>,
{
    extern "C" fn hunk_consumer(a1: i64, a2: i64, b1: i64, b2: i64, _priv: *mut c_void) -> c_int {
        let mut _priv = unsafe { (_priv as *mut Vec<Hunk>).as_mut() };
        let a1 = a1 as usize;
        let a2 = a2 as usize;
        let b1 = b1 as usize;
        let b2 = b2 as usize;
        if let Some(result) = _priv {
            result.push(Hunk {
                add: (b1..(b1 + b2)),
                remove: (a1..(a1 + a2)),
            });
        }
        return 0;
    }

    let old_text = old_text.as_ref();
    let mut old_mmfile = ffi::mmfile_t {
        ptr: old_text.as_ptr() as *const c_char,
        size: old_text.len() as i64,
    };
    let new_text = new_text.as_ref();
    let mut new_mmfile = ffi::mmfile_t {
        ptr: new_text.as_ptr() as *const c_char,
        size: new_text.len() as i64,
    };
    let xpp = ffi::xpparam_t { flags: 0 };
    let xecfg = ffi::xdemitconf_t {
        flags: 0,
        hunk_func: Some(hunk_consumer),
    };
    let mut result: Vec<Hunk> = Vec::new();
    let mut ecb = ffi::xdemitcb_t {
        priv_: &mut result as *mut Vec<Hunk> as *mut c_void,
    };

    unsafe {
        ffi::xdl_diff(&mut old_mmfile, &mut new_mmfile, &xpp, &xecfg, &mut ecb);
    }
    return result;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blocks() {
        // We're repeating the example from doctest because "buck test" doesn't run it.
        let a = "a\n b\n c\n d\n";
        let b = "a\n c\n d\n e\n";
        let c = "a\n b\n c\n d\n e\n";
        assert_eq!(
            diff_hunks(a, b),
            [
                Hunk {
                    add: 1..1,
                    remove: 1..2,
                },
                Hunk {
                    add: 3..4,
                    remove: 4..4,
                }
            ]
        );
        assert_eq!(
            diff_hunks(a, c),
            [Hunk {
                add: 4..5,
                remove: 4..4
            }]
        );
        let x = diff_hunks(b, c);
        assert_eq!(x[0].add.end, 2);
    }
}
