/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::ChangesetContext;
use anyhow::{anyhow, Context, Result};
use context::CoreContext;
use futures::{future::BoxFuture, FutureExt};
use mononoke_types::MPath;
#[allow(unused)]
use pathmatcher::TreeMatcher;

#[derive(Clone, Debug, PartialEq)]
pub enum SparseProfileEntry {
    Include(String),
    Exclude(String),
}

#[allow(unused)]
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
) -> BoxFuture<'a, Result<Vec<SparseProfileEntry>>> {
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
            .ok_or_else(|| anyhow!("{:?} not found", path))?;
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

#[allow(unused)]
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

#[allow(unused)]
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

#[allow(unused)]
fn makeglobrecursive(mut s: String) -> String {
    if s.ends_with('/') || s.is_empty() {
        s.push_str("**")
    } else {
        s.push_str("/**");
    }
    s
}
