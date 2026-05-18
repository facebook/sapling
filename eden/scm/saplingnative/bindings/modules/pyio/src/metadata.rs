/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use ::vfs::LiteMetadata;
use cpython::*;

const STAT_RESULT_LEN: i64 = 10;

py_class!(pub class metadata |py| {
    data inner: LiteMetadata;

    def __len__(&self) -> PyResult<usize> {
        Ok(STAT_RESULT_LEN as usize)
    }

    def __getitem__(&self, index: i64) -> PyResult<PyObject> {
        metadata_item(py, self.inner(py), index)
    }

    def __repr__(&self) -> PyResult<String> {
        Ok(metadata_repr(self.inner(py)))
    }

    @property
    def st_mode(&self) -> PyResult<u32> {
        Ok(self.inner(py).mode())
    }

    @property
    def st_size(&self) -> PyResult<u64> {
        Ok(self.inner(py).size())
    }

    @property
    def st_atime(&self) -> PyResult<i64> {
        Ok(system_time_to_timestamp_secs(self.inner(py).atime()))
    }

    @property
    def st_mtime(&self) -> PyResult<i64> {
        Ok(system_time_to_timestamp_secs(self.inner(py).mtime()))
    }

    @property
    def st_ctime(&self) -> PyResult<i64> {
        Ok(system_time_to_timestamp_secs(self.inner(py).ctime()))
    }

    @property
    def st_dev(&self) -> PyResult<u64> {
        Ok(self.inner(py).dev())
    }

    @property
    def st_ino(&self) -> PyResult<u64> {
        Ok(self.inner(py).ino())
    }

    @property
    def st_nlink(&self) -> PyResult<u64> {
        Ok(self.inner(py).nlink())
    }

    @property
    def st_uid(&self) -> PyResult<u32> {
        Ok(self.inner(py).uid())
    }

    @property
    def st_gid(&self) -> PyResult<u32> {
        Ok(self.inner(py).gid())
    }

    def is_file(&self) -> PyResult<bool> {
        Ok(self.inner(py).is_file())
    }

    def is_dir(&self) -> PyResult<bool> {
        Ok(self.inner(py).is_dir())
    }

    def is_symlink(&self) -> PyResult<bool> {
        Ok(self.inner(py).is_symlink())
    }

    def is_executable(&self) -> PyResult<bool> {
        Ok(self.inner(py).is_executable())
    }
});

fn metadata_item(py: Python, metadata: &LiteMetadata, index: i64) -> PyResult<PyObject> {
    let index = if index < 0 {
        STAT_RESULT_LEN + index
    } else {
        index
    };
    match index {
        0 => Ok(metadata.mode().to_py_object(py).into_object()),
        1 => Ok(metadata.ino().to_py_object(py).into_object()),
        2 => Ok(metadata.dev().to_py_object(py).into_object()),
        3 => Ok(metadata.nlink().to_py_object(py).into_object()),
        4 => Ok(metadata.uid().to_py_object(py).into_object()),
        5 => Ok(metadata.gid().to_py_object(py).into_object()),
        6 => Ok(metadata.size().to_py_object(py).into_object()),
        7 => Ok(system_time_to_timestamp_secs(metadata.atime())
            .to_py_object(py)
            .into_object()),
        8 => Ok(system_time_to_timestamp_secs(metadata.mtime())
            .to_py_object(py)
            .into_object()),
        9 => Ok(system_time_to_timestamp_secs(metadata.ctime())
            .to_py_object(py)
            .into_object()),
        _ => Err(PyErr::new::<exc::IndexError, _>(
            py,
            "metadata index out of range",
        )),
    }
}

fn metadata_repr(metadata: &LiteMetadata) -> String {
    format!(
        "stat_result(st_mode={}, st_ino={}, st_dev={}, st_nlink={}, st_uid={}, st_gid={}, st_size={}, st_atime={}, st_mtime={}, st_ctime={})",
        metadata.mode(),
        metadata.ino(),
        metadata.dev(),
        metadata.nlink(),
        metadata.uid(),
        metadata.gid(),
        metadata.size(),
        system_time_to_timestamp_secs(metadata.atime()),
        system_time_to_timestamp_secs(metadata.mtime()),
        system_time_to_timestamp_secs(metadata.ctime()),
    )
}

fn system_time_to_timestamp_secs(time: SystemTime) -> i64 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_secs().min(i64::MAX as u64) as i64,
        Err(err) => {
            let secs = err.duration().as_secs();
            if secs > i64::MAX as u64 {
                i64::MIN
            } else {
                -(secs as i64)
            }
        }
    }
}
