/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

pub fn prepare_prefix(input: &str) -> String {
    let mut escaped = input.to_string();
    escaped = escaped.replace('\\', "\\\\");
    escaped = escaped.replace('_', "\\_");
    escaped = escaped.replace('%', "\\%");
    escaped.push('%');
    escaped
}
