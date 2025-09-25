/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Arguments for File Specifiers

use anyhow::Result;
use clap::Args;
use clap::Subcommand;
use commit_id_types::CommitIdArgs;
use scs_client_raw::thrift;

use crate::ScscApp;
use crate::args::commit_id::resolve_commit_id;
use crate::args::path::PathArgs;
use crate::args::repo::RepoArgs;

#[derive(Subcommand, Clone)]
pub enum FileSpecifierType {
    /// Specify a file by its path in a commit
    ByCommitPath {
        #[clap(flatten)]
        repo_args: RepoArgs,
        #[clap(flatten)]
        commit_id_args: CommitIdArgs,
        #[clap(flatten)]
        path_args: PathArgs,
    },
    /// Specify a file by its ID (hex-encoded)
    ById {
        #[clap(flatten)]
        repo_args: RepoArgs,
        /// File ID as hex string
        #[clap(long, short)]
        id: String,
    },
    /// Specify a file by its SHA-1 content hash (hex-encoded)
    BySha1Hash {
        #[clap(flatten)]
        repo_args: RepoArgs,
        /// SHA-1 content hash as hex string
        #[clap(long)]
        sha1: String,
    },
    /// Specify a file by its SHA-256 content hash (hex-encoded)
    BySha256Hash {
        #[clap(flatten)]
        repo_args: RepoArgs,
        /// SHA-256 content hash as hex string
        #[clap(long)]
        sha256: String,
    },
}

#[derive(Args, Clone)]
pub struct FileSpecifierArgs {
    #[clap(subcommand)]
    pub specifier: FileSpecifierType,
}

impl FileSpecifierArgs {
    /// Convert this FileSpecifierArgs into a thrift::FileSpecifier
    pub async fn into_file_specifier(self, app: &ScscApp) -> Result<thrift::FileSpecifier> {
        match self.specifier {
            FileSpecifierType::ByCommitPath {
                repo_args,
                commit_id_args,
                path_args,
            } => {
                let repo = repo_args.into_repo_specifier();
                let conn = app.get_connection(Some(&repo.name))?;
                let commit_id = commit_id_args.into_commit_id();
                let id = resolve_commit_id(&conn, &repo, &commit_id).await?;
                let path = path_args.path;

                Ok(thrift::FileSpecifier::by_commit_path(
                    thrift::CommitPathSpecifier {
                        commit: thrift::CommitSpecifier {
                            repo,
                            id,
                            ..Default::default()
                        },
                        path,
                        ..Default::default()
                    },
                ))
            }
            FileSpecifierType::ById { repo_args, id } => {
                let repo = repo_args.into_repo_specifier();
                let binary_id = decode_hex(&id)?;

                Ok(thrift::FileSpecifier::by_id(thrift::FileIdSpecifier {
                    repo,
                    id: binary_id,
                    ..Default::default()
                }))
            }
            FileSpecifierType::BySha1Hash { repo_args, sha1 } => {
                let repo = repo_args.into_repo_specifier();
                let content_hash = decode_hex(&sha1)?;

                Ok(thrift::FileSpecifier::by_sha1_content_hash(
                    thrift::FileContentHashSpecifier {
                        repo,
                        content_hash,
                        ..Default::default()
                    },
                ))
            }
            FileSpecifierType::BySha256Hash { repo_args, sha256 } => {
                let repo = repo_args.into_repo_specifier();
                let content_hash = decode_hex(&sha256)?;

                Ok(thrift::FileSpecifier::by_sha256_content_hash(
                    thrift::FileContentHashSpecifier {
                        repo,
                        content_hash,
                        ..Default::default()
                    },
                ))
            }
        }
    }
}

/// Decode a hex string to binary, with proper error handling
fn decode_hex(hex_str: &str) -> Result<Vec<u8>> {
    let hex_str = hex_str.trim();
    if hex_str.len() % 2 != 0 {
        return Err(anyhow::anyhow!(
            "Hex string must have even number of characters, got: {}",
            hex_str
        ));
    }
    let mut binary = vec![0u8; hex_str.len() / 2];
    faster_hex::hex_decode(hex_str.as_bytes(), &mut binary)
        .map_err(|e| anyhow::anyhow!("Invalid hex string '{}': {}", hex_str, e))?;
    Ok(binary)
}
