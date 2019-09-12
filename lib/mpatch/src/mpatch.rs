// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use libc::ssize_t;
use mpatch_sys::*;
use std::os::raw::{c_char, c_void};
use std::ptr;

unsafe extern "C" fn get_next_link(deltas: *mut c_void, index: ssize_t) -> *mut mpatch_flist {
    let deltas = (deltas as *const Vec<&[u8]>).as_ref().unwrap();
    if index < 0 || index as usize >= deltas.len() {
        return ptr::null_mut();
    }

    let delta: &[u8] = deltas[index as usize];

    let mut res: *mut mpatch_flist = ptr::null_mut();
    if mpatch_decode(
        delta.as_ptr() as *const c_char,
        delta.len() as isize,
        &mut res,
    ) < 0
    {
        return ptr::null_mut();
    }

    return res;
}

pub fn get_full_text(base_text: &[u8], deltas: &Vec<&[u8]>) -> Result<Vec<u8>, &'static str> {
    // If there are no deltas, just return the full text portion
    if deltas.len() == 0 {
        return Ok(base_text.to_vec());
    }

    unsafe {
        let patch: *mut mpatch_flist = mpatch_fold(
            deltas as *const Vec<&[u8]> as *mut c_void,
            Some(get_next_link),
            0,
            deltas.len() as isize,
        );
        if patch.is_null() {
            return Err("mpatch failed to process the deltas");
        }

        let outlen = mpatch_calcsize(base_text.len() as isize, patch);
        if outlen < 0 {
            mpatch_lfree(patch);
            return Err("mpatch failed to calculate size");
        }

        let outlen = outlen as usize;
        let mut result: Vec<u8> = Vec::with_capacity(outlen);
        result.set_len(outlen);
        if mpatch_apply(
            result.as_mut_ptr() as *mut c_char,
            base_text.as_ptr() as *const c_char,
            base_text.len() as ssize_t,
            patch,
        ) < 0
        {
            mpatch_lfree(patch);
            return Err("mpatch failed to apply patches");
        }

        mpatch_lfree(patch);
        return Ok(result);
    }
}

#[cfg(test)]
mod tests {
    use super::get_full_text;

    #[test]
    fn no_deltas() {
        let base_text = b"hello";
        let full_text = get_full_text(&base_text[..], &vec![]).unwrap();
        assert_eq!(base_text, full_text.as_slice());
    }

    #[test]
    fn no_deltas_empty_base() {
        let base_text = b"";
        let full_text = get_full_text(&base_text[..], &vec![]).unwrap();
        assert_eq!(base_text, full_text.as_slice());
    }

    #[test]
    fn test_apply_delta() {
        let base_text = b"My data";
        let deltas: Vec<&[u8]> =
            vec![b"\x00\x00\x00\x03\x00\x00\x00\x03\x00\x00\x00\x0Adeltafied "];

        let full_text = get_full_text(&base_text[..], &deltas).unwrap();
        assert_eq!(b"My deltafied data", full_text[..].as_ref());
    }

    #[test]
    fn test_apply_deltas() {
        let base_text = b"My data";
        let deltas: Vec<&[u8]> = vec![
            b"\x00\x00\x00\x03\x00\x00\x00\x03\x00\x00\x00\x0Adeltafied ",
            b"\x00\x00\x00\x03\x00\x00\x00\x0D\x00\x00\x00\x10still deltafied ",
        ];

        let full_text = get_full_text(&base_text[..], &deltas).unwrap();
        assert_eq!(b"My still deltafied data", full_text[..].as_ref());
    }

    #[test]
    fn test_apply_invalid_deltas() {
        let base_text = b"My data";

        // Short delta
        let deltas: Vec<&[u8]> = vec![b"\x00\x03"];

        let full_text = get_full_text(&base_text[..], &deltas);
        assert!(full_text.is_err());

        // Short data
        let deltas: Vec<&[u8]> = vec![
            b"\x00\x00\x00\x03\x00\x00\x00\x03\x00\x00\x00\x0Adeltafied ",
            b"\x00\x00\x00\x03\x00\x00\x00\x03\x00\x00\x00\x0Adelta",
        ];

        let full_text = get_full_text(&base_text[..], &deltas);
        assert!(full_text.is_err());

        // Delta doesn't match base_text
        let deltas: Vec<&[u8]> =
            vec![b"\x00\x00\x00\xFF\x00\x00\x01\x00\x00\x00\x00\x0Adeltafied "];

        let full_text = get_full_text(&base_text[..], &deltas);
        assert!(full_text.is_err());
    }
}
