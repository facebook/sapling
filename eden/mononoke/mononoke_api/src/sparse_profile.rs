/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::errors::MononokeError;
use crate::ChangesetContext;
use crate::ChangesetFileOrdering;
use crate::MononokePath;
use anyhow::{anyhow, Context, Error, Result};
use blobstore::Loadable;
use cloned::cloned;
use context::CoreContext;
use futures::{future::BoxFuture, FutureExt, Stream, StreamExt, TryStreamExt};
use mononoke_types::{fsnode::FsnodeEntry, MPath};
use pathmatcher::{DirectoryMatch, Matcher, TreeMatcher};
use slog::info;
use types::RepoPath;

use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
pub enum SparseProfileEntry {
    Include(String),
    Exclude(String),
}

impl SparseProfileEntry {
    fn as_path(&self) -> String {
        match self {
            SparseProfileEntry::Include(s) => s.to_string(),
            SparseProfileEntry::Exclude(s) => s.to_string(),
        }
    }

    fn prefix(&self) -> &str {
        match self {
            SparseProfileEntry::Include(_) => "",
            SparseProfileEntry::Exclude(_) => "!",
        }
    }
}

pub fn parse_sparse_profile_content<'a>(
    ctx: &'a CoreContext,
    changeset: &'a ChangesetContext,
    path: &'a MPath,
) -> BoxFuture<'a, Result<Vec<SparseProfileEntry>, MononokeError>> {
    enum Section {
        Include,
        Exclude,
        Metadata,
    }

    async move {
        let path_with_content = changeset.path_with_content(path.clone())?;
        let file_ctx = path_with_content
            .file()
            .await?
            .ok_or_else(|| anyhow!("While parsing_sparse_profile_content {:?} not found", path))?;
        let content = file_ctx.content_concat().await?;

        let content =
            String::from_utf8(content.to_vec()).context("while converting content to utf8")?;

        let mut res = vec![];
        let mut section = Section::Include;
        for line in content.lines() {
            let line = line.trim();

            if line == "[include]" {
                section = Section::Include;
            } else if line == "[exclude]" {
                section = Section::Exclude;
            } else if line == "[metadata]" {
                section = Section::Metadata;
            } else if let Some(include_path) = line.strip_prefix("%include") {
                let include_path = MPath::new(include_path.trim())?;
                let included = parse_sparse_profile_content(ctx, changeset, &include_path).await?;
                res.extend(included);
            } else {
                if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                    continue;
                }
                match section {
                    Section::Include => {
                        res.push(SparseProfileEntry::Include(line.to_string()));
                    }
                    Section::Exclude => {
                        res.push(SparseProfileEntry::Exclude(line.to_string()));
                    }
                    Section::Metadata => {}
                };
            }
        }

        Ok(res)
    }
    .boxed()
}

pub(crate) fn build_tree_matcher(entries: Vec<SparseProfileEntry>) -> Result<TreeMatcher> {
    let mut rules_includes = vec![];
    let mut rules_excludes = vec![];
    for entry in entries {
        let globs = convert_to_globs(entry.as_path())
            .ok_or_else(|| anyhow!("bad sparse profile entry: {:?}", entry))?;
        for glob in globs {
            let rule = format!("{}{}", entry.prefix(), glob);
            match entry {
                SparseProfileEntry::Include(_) => rules_includes.push(rule),
                SparseProfileEntry::Exclude(_) => rules_excludes.push(rule),
            }
        }
    }

    let matcher =
        TreeMatcher::from_rules(rules_includes.into_iter().chain(rules_excludes.into_iter()))?;
    Ok(matcher)
}

fn convert_to_globs(s: String) -> Option<Vec<String>> {
    let (kind, pat) = match s.split_once(':') {
        Some((kind, pat)) => (kind, pat),
        None => {
            return Some(vec![makeglobrecursive(s)]);
        }
    };

    if kind == "re" {
        panic!(
            "Regular expression in sparse profiles config are discouraged.\n\
            Size analysis of such profiles is not implemented."
        )
    } else if kind == "glob" {
        let mut globs = vec![];
        for pat in pathmatcher::expand_curly_brackets(pat) {
            let pat = pathmatcher::normalize_glob(&pat);
            globs.push(makeglobrecursive(pat));
        }
        Some(globs)
    } else if kind == "path" {
        let pat = if pat == "." {
            String::new()
        } else {
            pathmatcher::plain_to_glob(pat)
        };
        Some(vec![makeglobrecursive(pat)])
    } else {
        Some(vec![])
    }
}

fn makeglobrecursive(mut s: String) -> String {
    if s.ends_with('/') || s.is_empty() {
        s.push_str("**")
    } else {
        s.push_str("/**");
    }
    s
}

pub async fn get_profile_size(
    ctx: &CoreContext,
    changeset: &ChangesetContext,
    path: &MPath,
) -> Result<Option<u64>, MononokeError> {
    let entries = match parse_sparse_profile_content(ctx, changeset, path).await {
        Err(e) => {
            info!(
                ctx.logger(),
                "error during parsing profile {}: {}",
                path.to_string(),
                e
            );
            return Ok(None);
        }
        Ok(entries) => entries,
    };

    let matcher = match build_tree_matcher(entries) {
        Err(e) => {
            info!(
                ctx.logger(),
                "error during building tree matcher for {}: {}",
                path.to_string(),
                e
            );
            return Ok(None);
        }
        Ok(m) => m,
    };

    Ok(Some(
        calculate_size(ctx, changeset, Arc::new(matcher)).await?,
    ))
}

async fn calculate_size(
    ctx: &CoreContext,
    changeset: &ChangesetContext,
    matcher: Arc<TreeMatcher>,
) -> Result<u64, MononokeError> {
    let root_fsnode_id = changeset.root_fsnode_id().await?;
    let root: Option<MPath> = None;
    let sizes = bounded_traversal::bounded_traversal_stream(
        256,
        vec![(root, *root_fsnode_id.fsnode_id())],
        |(path, fsnode_id)| {
            cloned!(ctx, matcher);
            let blobstore = changeset.repo().blob_repo().blobstore();
            async move {
                let mut size = 0;
                let mut next = vec![];
                let fsnode = fsnode_id.load(&ctx, blobstore).await?;
                for (base_name, entry) in fsnode.list() {
                    let path = MPath::join_opt_element(path.as_ref(), base_name);
                    let path_vec = path.to_vec();
                    let repo_path = RepoPath::from_utf8(&path_vec)?;
                    match entry {
                        FsnodeEntry::File(leaf) => {
                            if matcher.matches_file(repo_path)? {
                                size += leaf.size();
                            }
                        }
                        FsnodeEntry::Directory(tree) => {
                            match matcher.matches_directory(repo_path)? {
                                DirectoryMatch::Everything => {
                                    size += tree.summary().descendant_files_total_size;
                                }
                                DirectoryMatch::Nothing => {}
                                DirectoryMatch::ShouldTraverse => {
                                    next.push((Some(path), *tree.id()));
                                }
                            }
                        }
                    }
                }

                Result::<_, Error>::Ok((size, next))
            }
            .boxed()
        },
    )
    .try_collect::<Vec<_>>()
    .await?;
    Ok(sizes.iter().sum())
}

pub async fn get_all_profiles(
    changeset: &ChangesetContext,
) -> Result<impl Stream<Item = MPath>, MononokeError> {
    // TODO: read profile location from config
    let prefixes = vec![MononokePath::try_from("tools/scm/sparse")?];
    Ok(changeset
        .find_files(Some(prefixes), None, ChangesetFileOrdering::Unordered)
        .await?
        .filter_map(|path| async move {
            path.ok()?
                .into_mpath()
                .filter(|p| p.basename().as_ref() != b"README.md")
        }))
}
