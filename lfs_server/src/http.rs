// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use failure::Error;
use hyper::StatusCode;
use lazy_static::lazy_static;
use std::str::FromStr;

// Provide an easy way to map from Error -> Http code
pub struct HttpError {
    pub error: Error,
    pub status_code: StatusCode,
}

impl HttpError {
    pub fn e400<E: Into<Error>>(err: E) -> Self {
        Self {
            error: err.into(),
            status_code: StatusCode::BAD_REQUEST,
        }
    }

    pub fn e404<E: Into<Error>>(err: E) -> Self {
        Self {
            error: err.into(),
            status_code: StatusCode::NOT_FOUND,
        }
    }

    pub fn e500<E: Into<Error>>(err: E) -> Self {
        Self {
            error: err.into(),
            status_code: StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub fn e502<E: Into<Error>>(err: E) -> Self {
        Self {
            error: err.into(),
            status_code: StatusCode::BAD_GATEWAY,
        }
    }
}

lazy_static! {
    static ref GIT_LFS_MIME: mime::Mime =
        mime::Mime::from_str("application/vnd.git-lfs+json").unwrap();
}

pub fn git_lfs_mime() -> mime::Mime {
    GIT_LFS_MIME.clone()
}
