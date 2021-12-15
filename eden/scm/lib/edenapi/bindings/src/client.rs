/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Error;
use edenapi::BlockingResponse;
use edenapi::Builder;
use edenapi::EdenApi;
use edenapi_types::EdenApiServerError;
use edenapi_types::TreeEntry;
use libc::size_t;
use types::Key as ApiKey;

use crate::ptr_len_to_slice;
use crate::types::TreeAttributes;
use crate::EdenApiClient;
use crate::Key;
use crate::TreeEntryFetch;

type Client = Arc<dyn EdenApi>;

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
    keys: *const Key,
    keys_len: size_t,
    attrs: TreeAttributes,
) -> Result<Vec<Result<TreeEntry, EdenApiServerError>>, Error> {
    assert!(!client.is_null());
    let client: &Client = unsafe { &*client };
    let keys: &[Key] = unsafe { std::slice::from_raw_parts(keys, keys_len) };
    let keys: Vec<ApiKey> = keys
        .iter()
        .map(|k| k.try_into())
        .collect::<Result<Vec<ApiKey>, _>>()?;
    Ok(BlockingResponse::from_async(client.trees(keys, Some(attrs.into()))).map(|f| f.entries)?)
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
    let _ = (repo, repo_len);
    edenapi_trees_blocking(client, keys, keys_len, attrs).into()
}
