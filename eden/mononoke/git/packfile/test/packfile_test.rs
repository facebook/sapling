/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;
use std::sync::atomic::AtomicBool;

use bytes::BytesMut;
use flate2::Compression;
use flate2::write::ZlibDecoder;
use flate2::write::ZlibEncoder;
use futures::stream;
use git_types::BaseObject;
use git_types::GitPackfileBaseItem;
use git_types::PackfileItem;
use git_types::test_util::object_content_from_owned_object;
use git_types::thrift;
use gix_hash::ObjectId;
use gix_object::ObjectRef;
use gix_object::Tag;
use mononoke_macros::mononoke;
use packfile::bundle::BundleWriter;
use packfile::bundle::RefNaming;
use packfile::pack::DeltaForm;
use packfile::pack::PackfileWriter;
use quickcheck::quickcheck;
use tempfile::NamedTempFile;

async fn get_objects_stream(
    with_delta: bool,
) -> anyhow::Result<impl stream::Stream<Item = anyhow::Result<PackfileItem>>> {
    // Create a few Git objects
    let tag_object = object_content_from_owned_object(gix_object::Object::Tag(Tag {
        target: ObjectId::empty_tree(gix_hash::Kind::Sha1),
        target_kind: gix_object::Kind::Tree,
        name: "TreeTag".into(),
        tagger: None,
        message: "Tag pointing to a tree".into(),
        pgp_signature: None,
    }))?;
    let blob_object =
        object_content_from_owned_object(gix_object::Object::Blob(gix_object::Blob {
            data: "Some file content".as_bytes().to_vec(),
        }))?;
    let tree_object =
        object_content_from_owned_object(gix_object::Object::Tree(gix_object::Tree {
            entries: vec![gix_object::tree::Entry {
                mode: gix_object::tree::EntryKind::Blob.into(),
                filename: "JustAFile.txt".into(),
                oid: ObjectId::empty_blob(gix_hash::Kind::Sha1),
            }],
        }))?;
    let mut pack_items = vec![
        PackfileItem::new_base(tag_object.raw().clone()),
        PackfileItem::new_base(blob_object.raw().clone()),
        PackfileItem::new_base(tree_object.raw().clone()),
    ];
    if with_delta {
        let another_tag_object = object_content_from_owned_object(gix_object::Object::Tag(Tag {
            target: ObjectId::empty_tree(gix_hash::Kind::Sha1),
            target_kind: gix_object::Kind::Tree,
            name: "BlobTag".into(),
            tagger: None,
            message: "Tag pointing to a blob".into(),
            pgp_signature: None,
        }))?;
        let another_tag_hash = BaseObject::new(another_tag_object.raw().clone())?
            .hash()
            .to_owned();
        let tag_hash = BaseObject::new(tag_object.raw().clone())?.hash().to_owned();

        let raw_instructions =
            git_delta::git_delta(tag_object.raw(), another_tag_object.raw(), 1_000_000)?;
        let decompressed_size = raw_instructions.len() as u64;
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&raw_instructions)?;
        let compressed_instruction_bytes = encoder.finish()?;
        let pack_item = PackfileItem::new_delta(
            another_tag_hash,
            tag_hash,
            decompressed_size,
            compressed_instruction_bytes,
        );
        pack_items.push(anyhow::Ok(pack_item));
    }
    let objects_stream = stream::iter(pack_items);
    Ok(objects_stream)
}

#[mononoke::test]
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
    // Convert it into a packfile item
    BaseObject::new(object_content_from_owned_object(tag.into())?.raw().clone())
        .expect("Expected successful PackfileItem creation");
    Ok(())
}

#[mononoke::test]
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
    // Convert it into a packfile item
    let item = BaseObject::new(object_content_from_owned_object(tag.into())?.raw().clone())
        .expect("Expected successful PackfileItem creation");
    let mut encoded_bytes = BytesMut::new();
    item.write_encoded(&mut encoded_bytes, true)
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

#[mononoke::fbinit_test]
async fn validate_basic_packfile_generation() -> anyhow::Result<()> {
    let objects_stream = get_objects_stream(false).await?;
    let concurrency = 100;
    let mut packfile_writer =
        PackfileWriter::new(Vec::new(), 3, concurrency, DeltaForm::RefAndOffset);
    // Validate we are able to write the objects to the packfile without errors
    packfile_writer
        .write(objects_stream)
        .await
        .expect("Expected successful write of objects to packfile");
    // Validate we are able to finish writing to the packfile and generate the final checksum
    let checksum = packfile_writer.finish().await;
    assert!(checksum.is_ok());
    Ok(())
}

#[mononoke::fbinit_test]
async fn validate_packfile_generation_format() -> anyhow::Result<()> {
    // Create a few Git objects
    let objects_stream = get_objects_stream(false).await?;
    let concurrency = 100;
    let mut packfile_writer =
        PackfileWriter::new(Vec::new(), 3, concurrency, DeltaForm::RefAndOffset);
    // Validate we are able to write the objects to the packfile without errors
    packfile_writer
        .write(objects_stream)
        .await
        .expect("Expected successful write of objects to packfile");
    // Validate we are able to finish writing to the packfile and generate the final checksum
    let checksum = packfile_writer
        .finish()
        .await
        .expect("Expected successful checksum computation for packfile");
    // Capture the packfile size and number of objects
    let (num_entries, size) = (packfile_writer.num_entries, packfile_writer.size);
    // Retrieve the raw_writer (in this case Vec) back from the PackfileWriter
    let written_content = packfile_writer.into_write();
    // Write the packfile to disk
    let mut created_file = NamedTempFile::new()?;
    created_file.write_all(written_content.as_ref())?;
    // Open the written packfile
    let opened_packfile = gix_pack::data::File::at(created_file.path(), gix_hash::Kind::Sha1);
    // Validate that the packfile gets opened without error
    assert!(opened_packfile.is_ok());
    let opened_packfile = opened_packfile.expect("Expected successful opening of packfile");
    // Validate the number of objects in the packfile
    assert_eq!(opened_packfile.num_objects(), num_entries);
    // Validate the size of the packfile
    assert_eq!(opened_packfile.data_len(), size as usize);
    // Verify the checksum of the packfile
    let checksum_from_file = opened_packfile
        .verify_checksum(
            &mut gix_features::progress::Discard,
            &AtomicBool::new(false),
        )
        .expect("Expected successful checksum computation");
    // Verify the checksum matches the hash generated when computing the packfile
    assert_eq!(checksum, checksum_from_file);
    Ok(())
}

#[mononoke::fbinit_test]
async fn validate_staggered_packfile_generation() -> anyhow::Result<()> {
    let concurrency = 100;
    let mut packfile_writer =
        PackfileWriter::new(Vec::new(), 3, concurrency, DeltaForm::RefAndOffset);
    // Create Git objects and write them to a packfile one at a time
    let tag_object = object_content_from_owned_object(gix_object::Object::Tag(Tag {
        target: ObjectId::empty_tree(gix_hash::Kind::Sha1),
        target_kind: gix_object::Kind::Tree,
        name: "TreeTag".into(),
        tagger: None,
        message: "Tag pointing to a tree".into(),
        pgp_signature: None,
    }))?;
    // Validate we are able to write the object to the packfile without errors
    packfile_writer
        .write(stream::iter(vec![PackfileItem::new_base(
            tag_object.raw().clone(),
        )]))
        .await
        .expect("Expected successful write of object to packfile");
    let blob_object =
        object_content_from_owned_object(gix_object::Object::Blob(gix_object::Blob {
            data: "Some file content".as_bytes().to_vec(),
        }))?;
    // Validate we are able to write the object to the packfile without errors
    packfile_writer
        .write(stream::iter(vec![PackfileItem::new_base(
            blob_object.raw().clone(),
        )]))
        .await
        .expect("Expected successful write of object to packfile");
    let tree_object =
        object_content_from_owned_object(gix_object::Object::Tree(gix_object::Tree {
            entries: vec![gix_object::tree::Entry {
                mode: gix_object::tree::EntryKind::Blob.into(),
                filename: "JustAFile.txt".into(),
                oid: ObjectId::empty_blob(gix_hash::Kind::Sha1),
            }],
        }))?;
    // Validate we are able to write the object to the packfile without errors
    packfile_writer
        .write(stream::iter(vec![PackfileItem::new_base(
            tree_object.raw().clone(),
        )]))
        .await
        .expect("Expected successful write of object to packfile");

    // Validate we are able to finish writing to the packfile and generate the final checksum
    let checksum = packfile_writer
        .finish()
        .await
        .expect("Expected successful checksum computation for packfile");
    // Capture the packfile size and number of objects
    let (num_entries, size) = (packfile_writer.num_entries, packfile_writer.size);
    // Retrieve the raw_writer (in this case Vec) back from the PackfileWriter
    let written_content = packfile_writer.into_write();
    // Write the packfile to disk
    let mut created_file = NamedTempFile::new()?;
    created_file.write_all(written_content.as_ref())?;
    // Open the written packfile
    let opened_packfile = gix_pack::data::File::at(created_file.path(), gix_hash::Kind::Sha1);
    // Validate that the packfile gets opened without error
    assert!(opened_packfile.is_ok());
    let opened_packfile = opened_packfile.expect("Expected successful opening of packfile");
    // Validate the number of objects in the packfile
    assert_eq!(opened_packfile.num_objects(), num_entries);
    // Validate the size of the packfile
    assert_eq!(opened_packfile.data_len(), size as usize);
    // Verify the checksum of the packfile
    let checksum_from_file = opened_packfile
        .verify_checksum(
            &mut gix_features::progress::Discard,
            &AtomicBool::new(false),
        )
        .expect("Expected successful checksum computation");
    // Verify the checksum matches the hash generated when computing the packfile
    assert_eq!(checksum, checksum_from_file);
    Ok(())
}

#[mononoke::fbinit_test]
async fn validate_roundtrip_packfile_generation() -> anyhow::Result<()> {
    // Create a few Git objects
    let objects_stream = get_objects_stream(false).await?;
    let concurrency = 100;
    let mut packfile_writer =
        PackfileWriter::new(Vec::new(), 3, concurrency, DeltaForm::RefAndOffset);
    // Validate we are able to write the objects to the packfile without errors
    packfile_writer
        .write(objects_stream)
        .await
        .expect("Expected successful write of objects to packfile");
    // Validate we are able to finish writing to the packfile and generate the final checksum
    packfile_writer
        .finish()
        .await
        .expect("Expected successful checksum computation for packfile");
    // Retrieve the raw_writer (in this case Vec) back from the PackfileWriter
    let written_content = packfile_writer.into_write();
    // Write the packfile to disk
    let mut created_file = NamedTempFile::new()?;
    created_file.write_all(written_content.as_ref())?;
    // Open the written packfile
    let opened_packfile = gix_pack::data::File::at(created_file.path(), gix_hash::Kind::Sha1);
    // Validate that the packfile gets opened without error
    assert!(opened_packfile.is_ok());
    let opened_packfile = opened_packfile.expect("Expected successful opening of packfile");
    // Validate that we are able to iterate over the entries in the packfile
    for entry in opened_packfile
        .streaming_iter()
        .expect("Expected successful iteration of packfile entries")
    {
        // Validate the entry is a valid Git object
        let entry = entry.expect("Expected valid Git object in packfile entry");
        // Since we used only base objects, the packfile entries should all have is_base() set to true
        assert!(entry.header.is_base());
    }
    Ok(())
}

#[mononoke::fbinit_test]
async fn validate_delta_packfile_generation() -> anyhow::Result<()> {
    // Create a few Git objects along with delta variants
    let objects_stream = get_objects_stream(true).await?;
    let concurrency = 100;
    let mut packfile_writer =
        PackfileWriter::new(Vec::new(), 4, concurrency, DeltaForm::OnlyOffset);
    // Validate we are able to write the objects to the packfile without errors
    packfile_writer
        .write(objects_stream)
        .await
        .expect("Expected successful write of objects to packfile");
    // Validate we are able to finish writing to the packfile and generate the final checksum
    packfile_writer
        .finish()
        .await
        .expect("Expected successful checksum computation for packfile");
    // Retrieve the raw_writer (in this case Vec) back from the PackfileWriter
    let written_content = packfile_writer.into_write();
    // Write the packfile to disk
    let mut created_file = NamedTempFile::new()?;
    created_file.write_all(written_content.as_ref())?;
    // Open the written packfile
    let opened_packfile = gix_pack::data::File::at(created_file.path(), gix_hash::Kind::Sha1);
    // Validate that the packfile gets opened without error
    assert!(opened_packfile.is_ok());
    let opened_packfile = opened_packfile.expect("Expected successful opening of packfile");
    // Validate that we are able to iterate over the entries in the packfile
    for entry in opened_packfile
        .streaming_iter()
        .expect("Expected successful iteration of packfile entries")
    {
        // Validate the entry is a valid Git object
        entry.expect("Expected valid Git object in packfile entry");
    }
    Ok(())
}

#[mononoke::fbinit_test]
async fn validate_basic_bundle_generation() -> anyhow::Result<()> {
    // Create a few Git objects
    let objects_stream = get_objects_stream(false).await?;
    let refs = vec![(
        "HEAD".to_owned(),
        ObjectId::empty_tree(gix_hash::Kind::Sha1),
    )];
    // Validate we are able to successfully create BundleWriter
    let concurrency = 100;
    let mut bundle_writer = BundleWriter::new_with_header(
        Vec::new(),
        refs,
        Vec::new(),
        3,
        concurrency,
        DeltaForm::RefAndOffset,
        RefNaming::AsIs,
    )
    .await
    .expect("Expected successful creation of BundleWriter");
    // Validate we are able to successfully write objects to the bundle
    bundle_writer
        .write(objects_stream)
        .await
        .expect("Expected successful write of objects to bundle.");
    // Validate we are able to finish writing to the bundle
    bundle_writer
        .finish()
        .await
        .expect("Expected successful finish of bundle creation");
    Ok(())
}

#[mononoke::fbinit_test]
async fn validate_staggered_bundle_generation() -> anyhow::Result<()> {
    let refs = vec![(
        "HEAD".to_owned(),
        ObjectId::empty_tree(gix_hash::Kind::Sha1),
    )];
    // Validate we are able to successfully create BundleWriter
    let concurrency = 100;
    let mut bundle_writer = BundleWriter::new_with_header(
        Vec::new(),
        refs,
        Vec::new(),
        3,
        concurrency,
        DeltaForm::RefAndOffset,
        RefNaming::RenameToHeads,
    )
    .await
    .expect("Expected successful creation of BundleWriter");
    // Create a few Git objects
    let tag_object = object_content_from_owned_object(gix_object::Object::Tag(Tag {
        target: ObjectId::empty_tree(gix_hash::Kind::Sha1),
        target_kind: gix_object::Kind::Tree,
        name: "TreeTag".into(),
        tagger: None,
        message: "Tag pointing to a tree".into(),
        pgp_signature: None,
    }))?;
    // Validate we are able to write the object to the bundle without errors
    bundle_writer
        .write(stream::iter(vec![PackfileItem::new_base(
            tag_object.raw().clone(),
        )]))
        .await
        .expect("Expected successful write of object to bundle");
    let blob_object =
        object_content_from_owned_object(gix_object::Object::Blob(gix_object::Blob {
            data: "Some file content".as_bytes().to_vec(),
        }))?;
    // Validate we are able to write the object to the bundle without errors
    bundle_writer
        .write(stream::iter(vec![PackfileItem::new_base(
            blob_object.raw().clone(),
        )]))
        .await
        .expect("Expected successful write of object to bundle");
    let tree_object =
        object_content_from_owned_object(gix_object::Object::Tree(gix_object::Tree {
            entries: vec![],
        }))?;
    // Validate we are able to write the object to the bundle without errors
    bundle_writer
        .write(stream::iter(vec![PackfileItem::new_base(
            tree_object.raw().clone(),
        )]))
        .await
        .expect("Expected successful write of object to bundle");
    // Validate we are able to finish writing to the bundle
    bundle_writer
        .finish()
        .await
        .expect("Expected successful finish of bundle creation");
    Ok(())
}

quickcheck! {
    fn git_packfile_base_item_thrift_roundtrip(entry: GitPackfileBaseItem) -> bool {
        let thrift_entry: thrift::GitPackfileBaseItem = entry.clone().into();
        let from_thrift_entry: GitPackfileBaseItem = thrift_entry.try_into().expect("thrift roundtrips should always be valid");
        println!("entry: {:?}", entry);
        println!("entry_from_thrift: {:?}", from_thrift_entry);
        entry == from_thrift_entry
    }
}
