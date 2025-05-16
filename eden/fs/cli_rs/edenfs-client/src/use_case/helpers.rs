/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;

use anyhow::Result;
use anyhow::bail;

pub(crate) fn config_url(is_pub: bool) -> String {
    format!("https://{}/hg/config/", interngraph_host(is_pub))
}

fn interngraph_host(is_pub: bool) -> &'static str {
    if is_pub {
        "interngraph.internmc.facebook.com"
    } else {
        "interngraph.intern.facebook.com"
    }
}

pub(crate) fn get_http_config(
    is_pub: bool,
    proxy_sock_path: Option<&str>,
) -> Result<http_client::Config> {
    let mut http_config = http_client::Config::default();
    if is_pub {
        let proxy_sock = match proxy_sock_path {
            Some(path) => path.to_string(),
            None => bail!("no proxy_sock_path when fetching remote config in pub domain"),
        };

        let intern_host = interngraph_host(is_pub);

        http_config.unix_socket_path = Some(proxy_sock);
        http_config.unix_socket_domains = HashSet::from([intern_host.to_string()]);
    }
    Ok(http_config)
}
