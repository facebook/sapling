/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Benchmark commands.
use std::io::Write;

use anyhow::Error;
use anyhow::Result;
use futures::stream;
use futures::Stream;
use itertools::Itertools;
use serde::Serialize;

use crate::render::Render;

#[derive(Serialize)]
pub(crate) struct SummaryOutput {
    result: String,
    count: usize,
}

impl Render for SummaryOutput {
    type Args = ();

    fn render(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        write!(w, "{} times: {}\n", self.count, self.result)?;
        Ok(())
    }

    fn render_json(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        Ok(serde_json::to_writer(w, self)?)
    }
}

pub(crate) fn summary_output(
    results: impl Iterator<Item = Result<(), Error>>,
) -> impl Stream<Item = Result<SummaryOutput>> {
    let ret = results
        .map(|res| match res {
            Ok(_) => "OK".to_string(),
            Err(e) => format!("{:?}", e),
        })
        .sorted()
        .counts()
        .into_iter()
        .map(|(key, count)| {
            Ok(SummaryOutput {
                result: key.clone(),
                count,
            })
        });
    stream::iter(ret)
}
