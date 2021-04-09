/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::{convert::TryInto, path::PathBuf};

use anyhow::Error;
use libc::size_t;

use edenapi::{Builder, Client, EdenApiBlocking};
use edenapi_types::{EdenApiServerError, TreeEntry};
use types::Key as ApiKey;

use crate::{ptr_len_to_slice, types::TreeAttributes, EdenApiClient, Key, TreeEntryFetch};

fn edenapi_client_new(repository: *const u8, repository_len: size_t) -> Result<Client, Error> {
    let repository = unsafe { ptr_len_to_slice(repository, repository_len) }?;
    let repository: &str = std::str::from_utf8(repository)?;
    let owned_repo: PathBuf = repository.to_owned().into();

    let hg = owned_repo.join(".hg");
    let config = configparser::hg::load::<String, String>(Some(&hg), None)?;

    let client = Builder::from_config(&config)?.build()?;
    Ok(client)
}

#[no_mangle]
pub extern "C" fn rust_edenapi_client_new(
    repository: *const u8,
    repository_len: size_t,
) -> EdenApiClient {
    edenapi_client_new(repository, repository_len).into()
}

fn edenapi_trees_blocking(
    client: *mut Client,
    repo: *const u8,
    repo_len: size_t,
    keys: *const Key,
    keys_len: size_t,
    attrs: TreeAttributes,
) -> Result<Vec<Result<TreeEntry, EdenApiServerError>>, Error> {
    assert!(!client.is_null());
    let client: &Client = unsafe { &*client };
    let repo = unsafe { ptr_len_to_slice(repo, repo_len) }?;
    let repo: &str = std::str::from_utf8(repo)?;
    let keys: &[Key] = unsafe { std::slice::from_raw_parts(keys, keys_len) };
    let repo = repo.to_string();
    let keys: Vec<ApiKey> = keys
        .iter()
        .map(|k| k.try_into())
        .collect::<Result<Vec<ApiKey>, _>>()?;
    Ok(client
        .trees_blocking(repo, keys, Some(attrs.into()), None)
        .map(|f| f.entries)?)
}

#[no_mangle]
pub extern "C" fn rust_edenapi_trees_blocking(
    client: *mut Client,
    repo: *const u8,
    repo_len: size_t,
    keys: *const Key,
    keys_len: size_t,
    attrs: TreeAttributes,
) -> TreeEntryFetch {
    edenapi_trees_blocking(client, repo, repo_len, keys, keys_len, attrs).into()
}
