/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use mononoke_macros::mononoke;

use super::is_internal_only_ref;

#[mononoke::test]
fn commit_cloud_refs_are_internal() {
    for name in [
        "refs/commitcloud/upload",
        "refs/commitcloud/upload0",
        "refs/commitcloud/upload_1",
        "refs/commitcloud/upload/0123456789abcdef0123456789abcdef01234567",
    ] {
        assert!(
            is_internal_only_ref(name.as_bytes()),
            "expected {name} to be internal-only"
        );
    }
}

#[mononoke::test]
fn ordinary_refs_are_not_internal() {
    for name in [
        "refs/heads/main",
        "refs/tags/v1",
        "refs/notes/commits",
        "refs/pull/1/head",
        // Near misses: neither shares the commit cloud prefix.
        "refs/commitcloud-other/x",
        "refs/heads/refs/commitcloud/upload",
    ] {
        assert!(
            !is_internal_only_ref(name.as_bytes()),
            "expected {name} to be importable"
        );
    }
}

#[mononoke::test]
fn non_utf8_ref_names_do_not_panic() {
    assert!(!is_internal_only_ref(b"refs/heads/\xff\xfe"));
    assert!(is_internal_only_ref(b"refs/commitcloud/upload\xff"));
}
