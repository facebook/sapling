/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use format_util::git_sha1_digest;
use format_util::hg_sha1_digest;
use types::Id20;
use types::Parents;
use types::SerializationFormat;

use crate::Bytes;
use crate::InvalidHgId;

macro_rules! sized_hash {
    ($name: ident, $size: literal) => {
        paste::paste! {
            pub type $name = ::types::hash::AbstractHashType<[< $name TypeInfo >], $size>;

            pub struct [< $name TypeInfo >];

            impl ::types::hash::HashTypeInfo for [< $name TypeInfo >] {
                const HASH_TYPE_NAME: &'static str = stringify!($name);
            }
        }
    };
}

macro_rules! blake2_hash {
    ($name: ident) => {
        sized_hash!($name, 32);
    };
}

/// Check hash of file (blob) or tree. Unfortunately, it's hard to obtain
/// whether this is in Git or Hg format deep down this path (too many changes
/// are required). Right now, just try both formats. Remember the last "pass"
/// format so we can skip the "bad" format for the next check.
pub(crate) fn check_hash(
    data: &Bytes,
    parents: Parents,
    kind: &str,
    id: Id20,
) -> Result<(), InvalidHgId> {
    let order = match FORMAT_CHECK_ORDER.load(Ordering::Acquire) {
        0 => [SerializationFormat::Hg, SerializationFormat::Git],
        _ => [SerializationFormat::Git, SerializationFormat::Hg],
    };

    // Report the hash of the first format, which is likely the more desirable.
    let mut first_computed = id;
    for (i, format) in order.into_iter().enumerate() {
        let computed = match format {
            SerializationFormat::Hg => {
                let (p1, p2) = parents.into_nodes();
                hg_sha1_digest(data.as_ref(), &p1, &p2)
            }
            SerializationFormat::Git => git_sha1_digest(data.as_ref(), kind),
        };
        if i == 0 {
            first_computed = computed;
        }
        if computed == id {
            if i == 1 {
                // Swap order so the next check might skip the wrong format.
                let _ = FORMAT_CHECK_ORDER.fetch_xor(1, Ordering::AcqRel);
            }
            return Ok(());
        }
    }

    return Err(InvalidHgId {
        expected: id,
        computed: first_computed,
        parents,
        data: data.clone(),
    });
}

// 0: [hg, git]; _: [git, hg]
static FORMAT_CHECK_ORDER: AtomicUsize = AtomicUsize::new(0);
