/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use blackbox::event::Event;
use blackbox::json;
use blackbox::SessionId;
use clidispatch::errors;
use clidispatch::ReqCtx;

use super::define_flags;
use super::Repo;
use super::Result;

define_flags! {
    pub struct DumpTraceOpts {
        /// time range
        #[short('t')]
        time_range: String = "since 15 minutes ago",

        /// blackbox session id (overrides --time-range)
        #[short('s')]
        session_id: i64,

        /// output path (.txt, .json, .json.gz, .spans.json)
        #[short('o')]
        output_path: String,
    }
}

pub fn run(ctx: ReqCtx<DumpTraceOpts>, _repo: &mut Repo) -> Result<u8> {
    let entries = {
        let blackbox = blackbox::SINGLETON.lock();
        let session_ids = if ctx.opts.session_id != 0 {
            vec![SessionId(ctx.opts.session_id as u64)]
        } else if let Some(range) = hgtime::HgTime::parse_range(&ctx.opts.time_range) {
            // Blackbox uses milliseconds. HgTime uses seconds.
            let ratio = 1000;
            blackbox.session_ids_by_pattern(&json!({"start": {
                 "timestamp_ms": ["range", range.start.unixtime.saturating_mul(ratio), range.end.unixtime.saturating_mul(ratio)]
             }})).into_iter().collect()
        } else {
            return Err(
                errors::Abort("both --time-range and --session-id are invalid".into()).into(),
            );
        };
        blackbox.entries_by_session_ids(session_ids)
    };

    let mut tracing_data_list = Vec::new();
    for entry in entries {
        if let Event::TracingData { serialized } = entry.data {
            if let Ok(uncompressed) = zstd::stream::decode_all(&serialized.0[..]) {
                if let Ok(data) = mincode::deserialize(&uncompressed) {
                    tracing_data_list.push(data)
                }
            }
        }
    }
    let merged = tracing_collector::TracingData::merge(tracing_data_list);

    crate::run::write_trace(ctx.io(), &ctx.opts.output_path, &merged)?;

    Ok(0)
}

pub fn name() -> &'static str {
    "debugdumptrace"
}

pub fn doc() -> &'static str {
    "export tracing information"
}

pub fn synopsis() -> Option<&'static str> {
    None
}
