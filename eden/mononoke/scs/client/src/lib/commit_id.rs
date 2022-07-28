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

use anyhow::bail;
use anyhow::Error;
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
    ids: &BTreeMap<String, String>,
    schemes: &HashSet<String>,
    w: &mut dyn Write,
) -> Result<(), Error> {
    let ids: BTreeMap<_, _> = ids
        .iter()
        .filter(|(scheme, _id)| schemes.contains(*scheme))
        .collect();

    if let Ok(scheme) = schemes.iter().exactly_one() {
        if let Ok(id) = ids.values().exactly_one() {
            if let Some((header, _indent)) = header {
                write!(w, "{}: ", header)?;
            }
            write!(w, "{}", id)?;
        } else {
            bail!("{} does not have a '{}' id", requested, scheme);
        }
    } else {
        if ids.is_empty() {
            let mut schemes: Vec<_> = schemes.iter().map(AsRef::as_ref).collect();
            schemes.sort_unstable();
            bail!(
                "{} does not have any '{}' ids",
                requested,
                schemes.as_slice().join("', '")
            );
        }
        let mut prefix = "";
        if let Some((header, indent)) = header {
            write!(w, "{}:{}", header, separator)?;
            prefix = indent;
        }
        for (i, (scheme, id)) in ids.iter().enumerate() {
            if i > 0 {
                write!(w, "{}", separator)?;
            }
            write!(w, "{}{}={}", prefix, scheme, id)?;
        }
    }
    Ok(())
}
