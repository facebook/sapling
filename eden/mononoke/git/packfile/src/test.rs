/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![cfg(test)]

use std::io::Write;

use bytes::Bytes;
use bytes::BytesMut;
use flate2::write::ZlibDecoder;
use gix_hash::ObjectId;
use gix_object::Object;
use gix_object::ObjectRef;
use gix_object::Tag;

use crate::types::to_vec_bytes;
use crate::types::PackfileItem;

#[test]
fn validate_packitem_creation() -> anyhow::Result<()> {
    // Create a Git object
    let tag = Tag {
        target: ObjectId::empty_tree(gix_hash::Kind::Sha1),
        target_kind: gix_object::Kind::Tree,
        name: "TreeTag".into(),
        tagger: None,
        message: "Tag pointing to a tree".into(),
        pgp_signature: None,
    };
    // Get the bytes of the Git object
    let bytes =
        to_vec_bytes(&Object::Tag(tag)).expect("Expected successful Git object serialization");
    // Convert it into a packfile item
    PackfileItem::new(Bytes::from(bytes)).expect("Expected successful PackfileItem creation");
    Ok(())
}

#[test]
fn validate_packfile_item_encoding() -> anyhow::Result<()> {
    // Create a Git object
    let tag = Tag {
        target: ObjectId::empty_tree(gix_hash::Kind::Sha1),
        target_kind: gix_object::Kind::Tree,
        name: "TreeTag".into(),
        tagger: None,
        message: "Tag pointing to a tree".into(),
        pgp_signature: None,
    };
    // Get the bytes of the Git object
    let bytes =
        to_vec_bytes(&Object::Tag(tag)).expect("Expected successful Git object serialization");
    // Convert it into a packfile item
    let item =
        PackfileItem::new(Bytes::from(bytes)).expect("Expected successful PackfileItem creation");
    let mut encoded_bytes = BytesMut::new();
    item.write_encoded(&mut encoded_bytes)
        .expect("Expected successful encoding of packfile item");
    let encoded_bytes = encoded_bytes.freeze();
    // Decode the bytes and try to recreate the git object
    let mut decoded_bytes = Vec::new();
    let mut decoder = ZlibDecoder::new(decoded_bytes);
    decoder.write_all(encoded_bytes.as_ref())?;
    decoded_bytes = decoder.finish()?;
    // Validate the decoded bytes represent a valid Git object
    ObjectRef::from_loose(decoded_bytes.as_ref())
        .expect("Expected successful Git object creation from decoded bytes");
    Ok(())
}
