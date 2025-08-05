/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use mononoke_app::args::TLSArgs;
use url::Url;

pub struct EdenapiConfig {
    pub url: Url,
    pub tls_args: TLSArgs,
    pub http_proxy_host: Option<String>,
    pub http_no_proxy: Option<String>,
}
