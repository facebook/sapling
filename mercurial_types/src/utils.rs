// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use url::percent_encoding::{self, define_encode_set, USERINFO_ENCODE_SET};

define_encode_set! {
    // Python urllib also encodes ','
    pub HG_ENCODE_SET = [USERINFO_ENCODE_SET] | {','}
}

pub fn percent_encode(input: &str) -> String {
    // This encode set doesn't exactly match what Python's urllib does, but it's
    // close enough and importantly it encodes '=' which is the only important
    // one.
    percent_encoding::utf8_percent_encode(input, HG_ENCODE_SET).collect::<String>()
}
