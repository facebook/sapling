/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Compare a file with another file using headerless diffs

use std::io::Write;

use anyhow::Result;
use scs_client_raw::thrift;
use serde::Serialize;

use crate::ScscApp;
use crate::args::file_specifier::FileSpecifierArgs;
use crate::render::Render;

#[derive(clap::Parser)]
/// Compare a file with another file using headerless diffs
pub(super) struct CommandArgs {
    /// Base file specifier
    #[clap(flatten)]
    base_file: FileSpecifierArgs,
    /// ID of the other file to compare against (hex-encoded file ID)
    #[clap(long)]
    other_file_id: String,
    /// Number of lines of unified context around differences
    #[clap(long = "unified", short = 'U', default_value_t = 3)]
    context: i64,
}

#[derive(Serialize)]
struct FileDiffOutput {
    diff: thrift::Diff,
}

impl Render for FileDiffOutput {
    type Args = CommandArgs;

    fn render(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        // file.rs only returns headerless unified diffs (raw_diff format)
        match &self.diff {
            thrift::Diff::raw_diff(raw_diff) => {
                if raw_diff.is_binary {
                    writeln!(w, "Binary files differ")?;
                } else if let Some(ref diff_content) = raw_diff.raw_diff {
                    // Headerless unified diff content as bytes, convert to string
                    let diff_str = String::from_utf8_lossy(diff_content);
                    write!(w, "{}", diff_str)?;
                } else {
                    // No diff content means files are identical
                }
            }
            _ => {
                writeln!(w, "Unexpected diff format from file_diff")?;
            }
        }
        Ok(())
    }

    fn render_json(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        Ok(serde_json::to_writer(w, self)?)
    }
}

fn decode_file_id(hex_str: &str) -> Result<Vec<u8>> {
    let hex_str = hex_str.trim();
    if hex_str.len() % 2 != 0 {
        return Err(anyhow::anyhow!(
            "File ID must have even number of characters, got: {}",
            hex_str
        ));
    }
    let mut binary = vec![0u8; hex_str.len() / 2];
    faster_hex::hex_decode(hex_str.as_bytes(), &mut binary)
        .map_err(|e| anyhow::anyhow!("Invalid hex string '{}': {}", hex_str, e))?;
    Ok(binary)
}

pub(super) async fn run(app: ScscApp, args: CommandArgs) -> Result<()> {
    // File Diff only supports ID input for the "other" file.
    let other_file_id = decode_file_id(&args.other_file_id)?;

    let base_file_specifier = args.base_file.clone().into_file_specifier(&app).await?;

    let params = thrift::FileDiffParams {
        other_file_id,
        format: thrift::DiffFormat::RAW_DIFF, // file.rs only does headerless unified diff
        context: args.context,
        ..Default::default()
    };

    let repo_name = match &base_file_specifier {
        thrift::FileSpecifier::by_commit_path(spec) => &spec.commit.repo.name,
        thrift::FileSpecifier::by_id(spec) => &spec.repo.name,
        thrift::FileSpecifier::by_sha1_content_hash(spec) => &spec.repo.name,
        thrift::FileSpecifier::by_sha256_content_hash(spec) => &spec.repo.name,
        _ => return Err(anyhow::anyhow!("Unknown file specifier type")),
    };
    let conn = app.get_connection(Some(repo_name))?;

    let response = conn.file_diff(&base_file_specifier, &params).await?;

    app.target
        .render_one(
            &args,
            FileDiffOutput {
                diff: response.diff,
            },
        )
        .await
}
