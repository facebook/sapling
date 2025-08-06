/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;

const NETWORK_TYPE_HEADER: &str = "x-fb-validated-x2pauth-advice-subject_network_type";

fn is_vpn_less<GetHeaderFn>(headers: GetHeaderFn) -> Result<bool>
where
    GetHeaderFn: Fn(&str) -> Result<Option<String>>,
{
    Ok(
        match headers(NETWORK_TYPE_HEADER)?.map(|s| s.to_lowercase()) {
            Some(h) if h == "public" => true,
            _ => false,
        },
    )
}

pub fn is_client_untrusted<GetHeaderFn>(headers: GetHeaderFn) -> Result<bool>
where
    GetHeaderFn: Fn(&str) -> Result<Option<String>>,
{
    is_vpn_less(headers)
}
