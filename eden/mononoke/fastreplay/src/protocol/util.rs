/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use std::str::FromStr;

fn exclude_empty<'a>(e: &'a str) -> Option<&'a str> {
    if e.is_empty() { None } else { Some(e) }
}

pub fn split_separated_list<'a>(
    list: &'a str,
    separator: &'a str,
) -> impl Iterator<Item = &'a str> {
    list.split(separator).filter_map(exclude_empty)
}

pub fn extract_separated_list<I, C, E>(list: &str, separator: &str) -> Result<C, Error>
where
    I: FromStr<Err = E>,
    C: FromIterator<I>,
    Error: From<E>,
{
    let r = split_separated_list(list, separator)
        .map(I::from_str)
        .collect::<Result<_, _>>()?;
    Ok(r)
}
