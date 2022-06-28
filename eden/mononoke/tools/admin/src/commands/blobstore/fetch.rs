/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use blobstore::Blobstore;
use blobstore::BlobstoreGetData;
use bytes::Bytes;
use chrono::Local;
use chrono::TimeZone;
use clap::ArgEnum;
use clap::Args;
use cmdlib_displaying::hexdump;
use context::CoreContext;
use git_types::Tree as GitTree;
use mercurial_types::HgChangesetEnvelope;
use mercurial_types::HgFileEnvelope;
use mercurial_types::HgManifestEnvelope;
use mononoke_types::fsnode::Fsnode;
use mononoke_types::skeleton_manifest::SkeletonManifest;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ContentChunk;
use mononoke_types::FileContents;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

#[derive(Args)]
pub struct BlobstoreFetchArgs {
    /// Write raw blob bytes to the given filename instead of
    /// printing to stdout.
    #[clap(long, short = 'o', value_name = "FILE", parse(from_os_str))]
    output: Option<PathBuf>,

    /// Blobstore key to fetch.
    #[clap(required = true)]
    key: String,

    /// Don't show blob info header.
    #[clap(long, short = 'q')]
    quiet: bool,

    /// Decode as a particular type.
    #[clap(long, arg_enum, default_value = "auto")]
    decode_as: DecodeAs,
}

#[derive(ArgEnum, Copy, Clone, Eq, PartialEq)]
pub enum DecodeAs {
    Hex,
    Auto,
    Changeset,
    Content,
    ContentChunk,
    HgChangeset,
    HgManifest,
    HgFilenode,
    GitTree,
    SkeletonManifest,
    Fsnode,
    // TODO: Missing types, e.g. RedactionKeyList,  DeletedManifest,
    // FastlogBatch, FileUnode, ManifestUnode,
}

impl DecodeAs {
    fn from_key_prefix(key: &str) -> Option<Self> {
        for index in Some(0)
            .into_iter()
            .chain(key.match_indices('.').map(|(index, _)| index + 1))
        {
            for (prefix, auto_decode_as) in [
                ("changeset.", DecodeAs::Changeset),
                ("content.", DecodeAs::Content),
                ("chunk.", DecodeAs::ContentChunk),
                ("hgchangeset.", DecodeAs::HgChangeset),
                ("hgmanifest.", DecodeAs::HgManifest),
                ("hgfilenode.", DecodeAs::HgFilenode),
                ("git.tree.", DecodeAs::GitTree),
                ("skeletonmanifest.", DecodeAs::SkeletonManifest),
                ("fsnode.", DecodeAs::Fsnode),
            ] {
                if key[index..].starts_with(prefix) {
                    return Some(auto_decode_as);
                }
            }
        }
        None
    }
}

enum Decoded {
    None,
    Fail(String),
    Display(String),
    Hexdump(Bytes),
}

impl Decoded {
    fn try_display<T: std::fmt::Display, E: std::fmt::Display>(data: Result<T, E>) -> Decoded {
        match data {
            Ok(data) => Decoded::Display(data.to_string()),
            Err(err) => Decoded::Fail(err.to_string()),
        }
    }

    fn try_debug<T: std::fmt::Debug, E: std::fmt::Display>(data: Result<T, E>) -> Decoded {
        match data {
            Ok(data) => Decoded::Display(format!("{:#?}", data)),
            Err(err) => Decoded::Fail(err.to_string()),
        }
    }
}

fn decode(key: &str, data: BlobstoreGetData, mut decode_as: DecodeAs) -> Decoded {
    if decode_as == DecodeAs::Auto {
        if let Some(auto_decode_as) = DecodeAs::from_key_prefix(key) {
            decode_as = auto_decode_as;
        }
    }
    match decode_as {
        DecodeAs::Hex | DecodeAs::Auto => Decoded::None,
        DecodeAs::Changeset => Decoded::try_debug(BonsaiChangeset::from_bytes(data.as_raw_bytes())),
        DecodeAs::Content => match FileContents::from_encoded_bytes(data.into_raw_bytes()) {
            Ok(FileContents::Bytes(data)) => Decoded::Hexdump(data),
            Ok(FileContents::Chunked(chunked)) => Decoded::Display(format!("{:#?}", chunked)),
            Err(err) => Decoded::Fail(err.to_string()),
        },
        DecodeAs::ContentChunk => match ContentChunk::from_encoded_bytes(data.into_raw_bytes()) {
            Ok(chunk) => Decoded::Hexdump(chunk.into_bytes()),
            Err(err) => Decoded::Fail(err.to_string()),
        },
        DecodeAs::HgChangeset => Decoded::try_display(HgChangesetEnvelope::from_blob(data.into())),
        DecodeAs::HgManifest => Decoded::try_display(HgManifestEnvelope::from_blob(data.into())),
        DecodeAs::HgFilenode => Decoded::try_display(HgFileEnvelope::from_blob(data.into())),
        DecodeAs::GitTree => Decoded::try_display(GitTree::try_from(data)),
        DecodeAs::SkeletonManifest => {
            Decoded::try_debug(SkeletonManifest::from_bytes(data.into_raw_bytes().as_ref()))
        }
        DecodeAs::Fsnode => Decoded::try_debug(Fsnode::from_bytes(data.into_raw_bytes().as_ref())),
    }
}

pub async fn fetch(
    ctx: &CoreContext,
    blobstore: &dyn Blobstore,
    fetch_args: BlobstoreFetchArgs,
) -> Result<()> {
    let value = blobstore
        .get(ctx, &fetch_args.key)
        .await
        .context("Failed to fetch blob")?;

    match value {
        None => {
            writeln!(std::io::stderr(), "No blob exists for {}", fetch_args.key)?;
        }
        Some(value) => {
            if !fetch_args.quiet {
                writeln!(std::io::stdout(), "Key: {}", fetch_args.key)?;
                if let Some(ctime) = value.as_meta().ctime() {
                    writeln!(
                        std::io::stdout(),
                        "Ctime: {} ({})",
                        ctime,
                        Local.timestamp(ctime, 0)
                    )?;
                }
                if let Some(sizes) = value.as_meta().sizes() {
                    writeln!(
                        std::io::stdout(),
                        "Size: {} ({} compressed)",
                        value.len(),
                        sizes.unique_compressed_size
                    )?;
                } else {
                    writeln!(std::io::stdout(), "Size: {}", value.len())?;
                }
                writeln!(std::io::stdout())?;
            }
            if let Some(output) = fetch_args.output {
                let mut file = File::create(output)
                    .await
                    .context("Failed to create output file")?;
                file.write_all(value.as_raw_bytes())
                    .await
                    .context("Failed to write to output file")?;
                file.flush().await?;
            } else {
                let bytes = value.as_raw_bytes().clone();
                match decode(&fetch_args.key, value, fetch_args.decode_as) {
                    Decoded::Display(decoded) => {
                        writeln!(std::io::stdout(), "{}", decoded)?;
                    }
                    Decoded::Hexdump(data) => {
                        hexdump(std::io::stdout(), data)?;
                    }
                    Decoded::Fail(err) => {
                        writeln!(std::io::stderr(), "Failed to decode: {}", err)?;
                        // Fall back to dumping as raw hex
                        hexdump(std::io::stdout(), bytes)?;
                    }
                    Decoded::None => {
                        hexdump(std::io::stdout(), bytes)?;
                    }
                }
            }
        }
    }

    Ok(())
}
