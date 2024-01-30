/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;

use anyhow::ensure;
use clidispatch::ReqCtx;
use cliparser::define_flags;
use workingcopy::workingcopy::WorkingCopy;

use super::Repo;
use super::Result;

define_flags! {
    pub struct DebugMergeStateOpts {
        /// add fake mandatory record for testing (ADVANCED)
        add_unsupported_mandatory_record: bool = false,

        /// add fake advisory record for testing (ADVANCED)
        add_unsupported_advisory_record: bool = false,
    }
}

pub fn run(ctx: ReqCtx<DebugMergeStateOpts>, _repo: &mut Repo, wc: &mut WorkingCopy) -> Result<u8> {
    if ctx.opts.add_unsupported_mandatory_record || ctx.opts.add_unsupported_advisory_record {
        ensure!(std::env::var_os("TESTTMP").is_some(), "only for tests");

        let wc = wc.lock()?;

        let mut ms = wc.read_merge_state()?.unwrap_or_default();
        if ctx.opts.add_unsupported_mandatory_record {
            ms.add_raw_record(b'X', vec!["mandatory record".to_string()]);
        }
        if ctx.opts.add_unsupported_advisory_record {
            ms.add_raw_record(b'x', vec!["advisory record".to_string()]);
        }

        wc.write_merge_state(&ms)?;

        return Ok(0);
    }

    let mut io = ctx.io().output();

    let ms = match wc.read_merge_state() {
        Ok(Some(ms)) => ms,
        Ok(None) => {
            writeln!(io, "no merge state found")?;
            return Ok(0);
        }
        Err(err) => match err.downcast::<repostate::UnsupportedMergeRecords>() {
            Ok(bad) => bad.0,
            Err(err) => return Err(err),
        },
    };

    write!(io, "{:?}", ms)?;

    Ok(0)
}

pub fn aliases() -> &'static str {
    "debugmergestate"
}

pub fn doc() -> &'static str {
    "print merge state"
}

pub fn synopsis() -> Option<&'static str> {
    None
}
