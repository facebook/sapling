/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Helper library for rendering commit ids

use std::collections::BTreeMap;
use std::collections::HashSet;
use std::io::Write;

use anyhow::Error;
use anyhow::bail;
use itertools::Itertools;

/// Render a Commit ID, potentially in multiple schemes.
///
/// If the commit ID is in a single scheme, then it is rendered as-is. If the
/// commit ID is in multiple schemes, then it is rendered in each scheme,
/// prefixed by the name of the scheme, separated by the separator.
///
/// If header is not None, then the ID is prefixed by the header, and any
/// commit IDs with multiple schemes are indented with the indent.
pub(crate) fn render_commit_id(
    header: Option<(&str, &str)>,
    separator: &str,
    requested: &str,
    commit_ids: &BTreeMap<String, String>,
    schemes: &HashSet<String>,
    w: &mut dyn Write,
) -> Result<(), Error> {
    let ids: BTreeMap<_, _> = commit_ids
        .iter()
        .filter(|(scheme, _id)| {
            if schemes.is_empty() {
                // If no schemes were requested, get any non-bonsai id, which
                // should be the repo's default id scheme returned by the server
                *scheme != "bonsai"
            } else {
                schemes.contains(*scheme)
            }
        })
        .collect();

    match ids.iter().at_most_one() {
        Ok(None) => {
            let mut schemes: Vec<_> = schemes.iter().map(AsRef::as_ref).collect();
            schemes.sort_unstable();
            if schemes.is_empty() {
                bail!("{requested} does not have an id in the default scheme");
            } else {
                bail!(
                    "{requested} does not have any '{}' ids",
                    schemes.as_slice().join("', '")
                );
            }
        }
        Ok(Some((_, id))) => {
            if let Some((header, _indent)) = header {
                write!(w, "{header}: ")?;
            }
            write!(w, "{}", id)?;
        }
        Err(ids_iter) => {
            let mut prefix = "";
            if let Some((header, indent)) = header {
                write!(w, "{}:{}", header, separator)?;
                prefix = indent;
            }
            for (i, (scheme, id)) in ids_iter.enumerate() {
                if i > 0 {
                    write!(w, "{}", separator)?;
                }
                write!(w, "{}{}={}", prefix, scheme, id)?;
            }
        }
    }
    Ok(())
}
