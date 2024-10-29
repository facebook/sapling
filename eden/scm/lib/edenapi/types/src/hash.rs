/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use types::Id20;
use types::Parents;

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

pub(crate) fn check_hash(
    data: &Bytes,
    parents: Parents,
    kind: &str,
    id: Id20,
) -> Result<(), InvalidHgId> {
    let _ = kind;
    let computed = Id20::from_content(data, parents);
    if computed == id {
        Ok(())
    } else {
        Err(InvalidHgId {
            expected: id,
            computed,
            parents,
            data: data.clone(),
        })
    }
}
