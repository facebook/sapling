/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

pub(crate) fn internal_error(error: impl ToString) -> land_service_if::InternalError {
    land_service_if::InternalError {
        reason: error.to_string(),
        backtrace: None,
        source_chain: Vec::new(),
    }
}
