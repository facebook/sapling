use anyhow::Error;
use std::os::raw::c_ulong;
use std::os::raw::c_void;

extern {
    fn git_delta_from_buffers(
        src_buf: *const c_void,
        src_len: c_ulong,
        trg_buf: *const c_void,
        trg_len: c_ulong,
        delta_size: *mut c_ulong,
        max_delta_size: c_ulong,
    ) -> *mut u8;
}

pub fn git_delta(src: &[u8], trg: &[u8], max_delta_size: usize) -> Result<Vec<u8>, Error> {
    let mut out_len = 0 as c_ulong;
    let src_len = src.len().try_into()?;
    let trg_len = trg.len().try_into()?;
    unsafe {
        let out = git_delta_from_buffers(
            src.as_ptr() as *const c_void,
            src_len,
            trg.as_ptr() as *const c_void,
            trg_len,
            &mut out_len,
            max_delta_size.try_into()?,
        );
        if out.is_null() {
            return Err(Error::msg("git_delta_from_buffers returned null"));
        }
        let raw_delta =  Vec::from_raw_parts(out, out_len.try_into()?, out_len.try_into()?);
        Ok(raw_delta)
    }
}

#[cfg(test)]
mod tests {
    use crate::git_delta;
    #[test]
    fn smoke_test() {
        assert!(git_delta(b"a", b"b", 40).is_ok());
    }

}
