/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::sync::LazyLock;

use minibytes::Bytes;

/// Generate `Bytes` with given length. Useful for dummy file contents.
pub fn generate_file_content_of_length(len: usize) -> Bytes {
    const LOREM_IPSUM: &str = r#"Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do
eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad
minim veniam, quis nostrud exercitation ullamco laboris nisi ut
aliquip ex ea commodo consequat. Duis aute irure dolor in
reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla
pariatur. Excepteur sint occaecat cupidatat non proident, sunt in
culpa qui officia deserunt mollit anim id est laborum.

"#;
    // Avoid allocation for small blobs.
    static LOREM_IPSUM_LONG: LazyLock<Bytes> = LazyLock::new(|| {
        const SIZE: usize = 4_000_000;
        let long_text = LOREM_IPSUM.repeat(SIZE / LOREM_IPSUM.len() + 1);
        Vec::from(long_text).into()
    });
    if len <= LOREM_IPSUM_LONG.len() {
        LOREM_IPSUM_LONG.slice(..len)
    } else {
        let mut result = Vec::with_capacity(len);
        while result.len() < len {
            let wanted_len = (len - result.len()).min(LOREM_IPSUM_LONG.len());
            result.extend_from_slice(&LOREM_IPSUM_LONG[..wanted_len]);
        }
        result.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_content_matches_requested_length() {
        for len in [
            0, 1, 2, 10, 50, 100, 500, 1000, 2000, 5000, 3_999_999, 4_000_000, 4_000_001,
            16_000_000, 20_000_000,
        ] {
            let blob = generate_file_content_of_length(len);
            assert_eq!(blob.len(), len);
        }
    }
}
