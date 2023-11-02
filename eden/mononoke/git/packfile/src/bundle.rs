/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt::Display;

use anyhow::Result;
use futures::Stream;
use gix_hash::ObjectId;
use tokio::io::AsyncWrite;
use tokio::io::AsyncWriteExt;

use crate::pack::DeltaForm;
use crate::pack::PackfileWriter;
use crate::types::PackfileItem;

/// The message/comment associated with the pre-requisite objects
const BUNDLE_PREREQ_MSG: &str = "bundled object";

/// Enum representing the supported bundle versions
/// Currently only version 2 is supported.
pub enum BundleVersion {
    V2,
}

impl Display for BundleVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BundleVersion::V2 => f.write_str("# v2 git bundle\n"),
        }
    }
}

/// Struct responsible for writing a Git bundle with format https://git-scm.com/docs/bundle-format
/// to the underlying writer.
pub struct BundleWriter<T>
where
    T: AsyncWrite + Unpin,
{
    /// List of objects that are NOT included in the bundle but are
    /// required to be present for unbundling to work.
    pub prereqs: Option<Vec<ObjectId>>,
    /// The version of bundle format
    pub version: BundleVersion,
    /// List of ref-names with the commits IDs that they point to
    pub refs: Vec<(String, ObjectId)>,
    /// Packfile writer created over the underlying raw writer
    pub pack_writer: PackfileWriter<T>,
}

#[allow(dead_code)]
impl<T: AsyncWrite + Unpin> BundleWriter<T> {
    /// Create a new BundleWriter instance with the header of the bundle written to the
    /// underlying writer.
    pub async fn new_with_header(
        mut writer: T,
        refs: Vec<(String, ObjectId)>,
        prereqs: Option<Vec<ObjectId>>,
        num_objects: u32,
        concurrency: usize,
        delta_form: DeltaForm,
    ) -> Result<Self> {
        // Append the bundle header
        writer
            .write_all(format!("{}", BundleVersion::V2).as_bytes())
            .await?;
        // Append the pre-requisite objects, if present
        if let Some(ref prereqs) = prereqs {
            for prereq in prereqs {
                writer
                    .write_all(format!("-{} {}\n", prereq, BUNDLE_PREREQ_MSG).as_bytes())
                    .await?;
            }
        }
        // Append the refs
        for (ref_name, id) in &refs {
            writer
                .write_all(format!("{} {}\n", id, ref_name).as_bytes())
                .await?;
        }
        // Newline before starting packfile
        writer.write_all(b"\n").await?;
        let pack_writer = PackfileWriter::new(writer, num_objects, concurrency, delta_form);
        Ok(Self {
            version: BundleVersion::V2,
            refs,
            prereqs,
            pack_writer,
        })
    }

    /// Write the stream of input items to the bundle
    pub async fn write(
        &mut self,
        objects_stream: impl Stream<Item = Result<PackfileItem>>,
    ) -> Result<()> {
        self.pack_writer.write(objects_stream).await
    }

    /// Finish the bundle and flush it to the underlying writer
    /// returning the checksum of the written packfile
    pub async fn finish(&mut self) -> Result<ObjectId> {
        self.pack_writer.finish().await
    }

    /// Consumes the instance after writing the bundle and returns
    /// the underlying raw writer.
    pub fn into_write(self) -> T {
        self.pack_writer.into_write()
    }
}
