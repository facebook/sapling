/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use gotham_ext::middleware::post_request::PostResponseInfo;
use hyper::StatusCode;

pub fn log_ods3(
    _info: &PostResponseInfo,
    _status: &StatusCode,
    _method: String,
    _method_variants: String,
    _repo: String,
    _request_load: Option<f64>,
) {
}
