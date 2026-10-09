/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::time::Instant;

use anyhow::Context;
use anyhow::Result;
use gitimport_session_protocol::MAX_FRAME_BYTES;
use gitimport_session_protocol::Operation;
use gitimport_session_protocol::ProtocolError;
use gitimport_session_protocol::Response;
use gitimport_session_protocol::decode_request;
use gitimport_session_protocol::encode_response;
use gix_hash::ObjectId;
use import_tools::GitRef;
use mononoke_app::MononokeApp;
use mononoke_app::args::AsRepoArg;
use tokio::io::AsyncBufRead;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;

use crate::GitimportArgs;
use crate::GitimportSubcommand;
use crate::ImportExecution;
use crate::ImportLifecycleTiming;
use crate::log_import_phase;
use crate::repo::Repo;
use crate::run_import;

/// Fixed diagnostic categories that may cross the persistent session boundary.
#[derive(Clone, Copy, Debug, thiserror::Error)]
pub(crate) enum SessionFailure {
    #[error("unsupported_options")]
    UnsupportedOptions,
    #[error("repository_config_failed")]
    RepositoryConfig,
    #[error("request_read_failed")]
    RequestRead,
    #[error("incomplete_frame")]
    IncompleteFrame,
    #[error("frame_too_large")]
    FrameTooLarge,
    #[error("repository_mismatch")]
    RepositoryMismatch,
    #[error("ref_outside_scope")]
    RefOutsideScope,
    #[error("snapshot_mismatch")]
    SnapshotMismatch,
    #[error("response_write_failed")]
    ResponseWrite,
    #[cfg(fbcode_build)]
    #[error("identity_failed")]
    Identity,
    #[error("open_repo_failed")]
    OpenRepo,
    #[error("lfs_setup_failed")]
    LfsSetup,
    #[error("read_git_failed")]
    ReadGit,
    #[error("discover_commits_failed")]
    DiscoverCommits,
    #[error("import_contents_failed")]
    ImportContents,
    #[error("resolve_refs_failed")]
    ResolveRefs,
    #[error("open_managed_repo_failed")]
    OpenManagedRepo,
    #[error("publication_failed")]
    Publication,
}

/// Add a safe diagnostic marker only for persistent requests.
pub(crate) trait SessionResultExt<T> {
    fn session_context(self, persistent: bool, failure: SessionFailure) -> Result<T>;
}

impl<T, E: Into<anyhow::Error>> SessionResultExt<T> for std::result::Result<T, E> {
    fn session_context(self, persistent: bool, failure: SessionFailure) -> Result<T> {
        self.map_err(|error| {
            let error: anyhow::Error = error.into();
            if persistent {
                error.context(failure)
            } else {
                error
            }
        })
    }
}

/// Preserve only known, static diagnostics; never retain the original error chain.
pub(crate) fn sanitize_error(error: anyhow::Error) -> anyhow::Error {
    let reason = if let Some(protocol_error) = error.downcast_ref::<ProtocolError>() {
        match protocol_error {
            ProtocolError::InvalidFrame => "invalid_frame",
            ProtocolError::InvalidVersion => "unsupported_version",
            ProtocolError::InvalidRequestId => "invalid_request_id",
            ProtocolError::InvalidRepoName => "invalid_repository_name",
            ProtocolError::InvalidRefs => "invalid_refs",
            ProtocolError::InvalidStatus => "invalid_response_status",
        }
        .to_owned()
    } else if let Some(failure) = error.downcast_ref::<SessionFailure>() {
        failure.to_string()
    } else {
        "unclassified".to_owned()
    };
    anyhow::anyhow!("Persistent import session failed: {reason}")
}

fn check_options(args: &GitimportArgs) -> Result<()> {
    anyhow::ensure!(
        matches!(&args.subcommand, GitimportSubcommand::Incremental)
            && !args.include_refs.is_empty()
            && args.exclude_refs.is_empty()
            && args.generate_bookmarks
            && args.suppress_ref_mapping
            && !args.cleanup_mononoke_bookmarks
            && !args.reupload_commits
            && !args.allow_dangling_lfs_pointers
            && !args.bypass_derived_data_backfilling
            && !args.discard_submodules
            && !args.allow_content_refs
            && !args.derive_hg,
        SessionFailure::UnsupportedOptions
    );
    Ok(())
}

/// Match the original ref object, including the outermost annotated tag object.
pub(crate) fn check_ref_snapshot(
    actual: &BTreeMap<GitRef, ObjectId>,
    expected: &BTreeMap<String, String>,
) -> Result<()> {
    anyhow::ensure!(
        actual.len() == expected.len(),
        SessionFailure::SnapshotMismatch
    );
    for (git_ref, commit) in actual {
        let name = std::str::from_utf8(&git_ref.name).context(SessionFailure::SnapshotMismatch)?;
        let oid = git_ref.metadata.maybe_tag_id.unwrap_or(*commit);
        anyhow::ensure!(
            expected
                .get(name)
                .is_some_and(|sha| sha == &oid.to_hex().to_string()),
            SessionFailure::SnapshotMismatch
        );
    }
    Ok(())
}

async fn read_frame(reader: &mut (impl AsyncBufRead + Unpin)) -> Result<Option<Vec<u8>>> {
    let mut frame = Vec::new();
    loop {
        let available = reader
            .fill_buf()
            .await
            .context(SessionFailure::RequestRead)?;
        if available.is_empty() {
            anyhow::ensure!(frame.is_empty(), SessionFailure::IncompleteFrame);
            return Ok(None);
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let count = newline.map_or(available.len(), |index| index + 1);
        anyhow::ensure!(
            count <= MAX_FRAME_BYTES - frame.len(),
            SessionFailure::FrameTooLarge
        );
        frame.extend_from_slice(&available[..count]);
        reader.consume(count);
        if newline.is_some() {
            return Ok(Some(frame));
        }
    }
}

pub(crate) async fn run(
    app: &MononokeApp,
    args: &GitimportArgs,
    mut phase_started: Instant,
    lifecycle_timing: &ImportLifecycleTiming,
) -> Result<()> {
    check_options(args)?;
    let (repo_name, _) = app
        .repo_config(args.repo_args.as_repo_arg())
        .context(SessionFailure::RepositoryConfig)?;
    let mut reader = BufReader::new(tokio::io::stdin());
    let mut output = tokio::io::stdout();
    let mut manager = None;
    let mut first_request = true;
    while let Some(frame) = read_frame(&mut reader).await? {
        let request = decode_request(&frame)?;
        let (current_repo_name, _) = app
            .repo_config(args.repo_args.as_repo_arg())
            .context(SessionFailure::RepositoryConfig)?;
        anyhow::ensure!(
            request.repo_name == repo_name && current_repo_name == repo_name,
            SessionFailure::RepositoryMismatch
        );
        anyhow::ensure!(
            request
                .refs
                .keys()
                .all(|name| args.include_refs.contains(name)),
            SessionFailure::RefOutsideScope
        );
        let response = encode_response(&Response::for_request(&request))?;
        if !first_request {
            phase_started = Instant::now();
        }
        first_request = false;
        match request.operation {
            Operation::Warm => {
                let repo: Repo = app
                    .open_repo(&args.repo_args)
                    .await
                    .context(SessionFailure::OpenRepo)?;
                drop(repo);
                log_import_phase(args.log_import_phases, "open_repo", &mut phase_started);
            }
            Operation::Import => {
                let request_timing = ImportLifecycleTiming::default();
                run_import(
                    app,
                    args,
                    phase_started,
                    ImportExecution {
                        expected_refs: Some(&request.refs),
                        lifecycle_timing: &request_timing,
                        session_manager: Some(&mut manager),
                    },
                )
                .await?;
                request_timing.log_async_cleanup(args.log_import_phases);
            }
        }
        output
            .write_all(&response)
            .await
            .context(SessionFailure::ResponseWrite)?;
        output
            .flush()
            .await
            .context(SessionFailure::ResponseWrite)?;
    }
    drop(manager);
    let _ = lifecycle_timing
        .runtime_shutdown_started
        .set(Instant::now());
    Ok(())
}

#[cfg(test)]
mod tests {
    use mononoke_macros::mononoke;

    use super::*;

    const PRIVATE_DIAGNOSTIC: &str =
        "token=fake-private-token https://example.invalid/object?signature=fake-signed-url";

    fn assert_sanitized(error: anyhow::Error, reason: &str) {
        let error = sanitize_error(error);
        assert_eq!(
            error.to_string(),
            format!("Persistent import session failed: {reason}")
        );
        assert_eq!(error.chain().count(), 1);
        for rendered in [
            format!("{error}"),
            format!("{error:#}"),
            format!("{error:?}"),
        ] {
            assert!(!rendered.contains("fake-private-token"));
            assert!(!rendered.contains("fake-signed-url"));
            assert!(!rendered.contains("example.invalid"));
        }
    }

    #[mononoke::test]
    fn sanitized_stages_discard_private_error_chains() {
        for stage in [
            SessionFailure::OpenRepo,
            SessionFailure::ImportContents,
            SessionFailure::Publication,
        ] {
            let error = anyhow::anyhow!(PRIVATE_DIAGNOSTIC)
                .context(stage)
                .context(PRIVATE_DIAGNOSTIC);
            assert_sanitized(error, &stage.to_string());
        }
        assert_sanitized(anyhow::anyhow!(PRIVATE_DIAGNOSTIC), "unclassified");
    }

    #[mononoke::test]
    fn one_shot_context_preserves_the_original_error_chain() {
        let original = anyhow::anyhow!(PRIVATE_DIAGNOSTIC)
            .context("original operation")
            .context("original outer context");
        let rendered = format!("{original:#}");
        let chain = original
            .chain()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        let result: Result<()> = Err(original);
        let error = result
            .session_context(false, SessionFailure::ImportContents)
            .unwrap_err();
        assert_eq!(format!("{error:#}"), rendered);
        assert_eq!(
            error.chain().map(ToString::to_string).collect::<Vec<_>>(),
            chain
        );
        assert!(error.downcast_ref::<SessionFailure>().is_none());

        let result: Result<()> = Err(error);
        assert_sanitized(
            result
                .session_context(true, SessionFailure::ImportContents)
                .unwrap_err(),
            "import_contents_failed",
        );
    }

    #[mononoke::test]
    fn sanitized_protocol_causes_remain_distinct() {
        for (error, reason) in [
            (ProtocolError::InvalidFrame, "invalid_frame"),
            (ProtocolError::InvalidVersion, "unsupported_version"),
            (ProtocolError::InvalidRequestId, "invalid_request_id"),
            (ProtocolError::InvalidRepoName, "invalid_repository_name"),
            (ProtocolError::InvalidRefs, "invalid_refs"),
            (ProtocolError::InvalidStatus, "invalid_response_status"),
        ] {
            assert_sanitized(anyhow::anyhow!(PRIVATE_DIAGNOSTIC).context(error), reason);
        }
    }

    #[mononoke::test]
    fn ref_snapshot_mismatch_keeps_its_safe_category() {
        let expected = BTreeMap::from([("refs/heads/main".to_owned(), "1".repeat(40))]);
        let error = check_ref_snapshot(&BTreeMap::new(), &expected).unwrap_err();
        assert_sanitized(error, "snapshot_mismatch");
    }
}
