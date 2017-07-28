// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Common types used by both decode and encode code.

use std::collections::HashMap;

#[derive(Debug, Eq, PartialEq)]
pub struct StreamHeader {
    // Stream parameters are specified as using a "simple textual format", which we
    // take to be valid UTF-8-encoded strings.
    pub m_stream_params: HashMap<String, String>,
    pub a_stream_params: HashMap<String, String>,
}
