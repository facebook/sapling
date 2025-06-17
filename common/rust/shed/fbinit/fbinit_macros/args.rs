/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under both the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree and the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree.
 */

use syn::Ident;
use syn::LitInt;
use syn::meta::ParseNestedMeta;
use syn::parse::Error;
use syn::parse::Result;

#[derive(Default)]
pub struct Args {
    pub disable_fatal_signals: DisableFatalSignals,
    pub tokio_workers: Option<usize>,
}

#[derive(Default)]
pub enum DisableFatalSignals {
    #[default]
    Default,
    None,
    SigtermOnly,
    All,
}

impl Args {
    pub fn parse(&mut self, meta: ParseNestedMeta) -> Result<()> {
        if meta.path.is_ident("disable_fatal_signals") {
            let ident: Ident = meta.value()?.parse()?;
            self.disable_fatal_signals = match ident.to_string().as_str() {
                "none" => DisableFatalSignals::None,
                "default" => DisableFatalSignals::Default,
                "sigterm_only" => DisableFatalSignals::SigtermOnly,
                "all" => DisableFatalSignals::All,
                _ => {
                    return Err(Error::new(
                        ident.span(),
                        "expected `none`, `default`, `sigterm_only`, or `all`",
                    ));
                }
            };
            Ok(())
        } else if meta.path.is_ident("worker_threads") {
            let lit: LitInt = meta.value()?.parse()?;
            let tokio_workers: usize = lit.base10_parse()?;
            self.tokio_workers = Some(tokio_workers);
            Ok(())
        } else {
            Err(meta.error("unrecognized fbinit attribute"))
        }
    }
}
