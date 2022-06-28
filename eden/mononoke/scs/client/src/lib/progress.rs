/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Adapters for rendering progress

use std::time::Duration;

use crate::render::Render;
use crate::render::RenderStream;
use anyhow::Error;
use clap::App;
use clap::Arg;
use clap::ArgMatches;
use futures::stream::Stream;
use futures::stream::StreamExt;
use futures::stream::{self};
use std::io::Write;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProgressOutput {
    message: String,
    value: u64,
    total: u64,
}

const ARG_NO_PROGRESS: &str = "NO_PROGRESS";

/// Add arguments to specify a set of commit identity schemes.
pub(crate) fn add_progress_args<'a, 'b>(app: App<'a, 'b>) -> App<'a, 'b> {
    app.arg(
        Arg::with_name(ARG_NO_PROGRESS)
            .long("no-progress")
            .help("Don't show the progress bar"),
    )
}

impl ProgressOutput {
    pub(crate) fn new(message: String, value: u64, total: u64) -> Self {
        Self {
            message,
            value,
            total,
        }
    }
}

struct ProgressSuspend;

struct ProgressClear;

enum ProgressItem {
    Item(Result<Box<dyn Render>, Error>),
    Complete,
    Timer,
}

pub(crate) fn progress_renderer<S, F>(
    matches: &ArgMatches,
    render_stream: S,
    get_progress: F,
) -> RenderStream
where
    S: Stream<Item = Result<Box<dyn Render>, Error>> + Send + 'static,
    F: Fn() -> ProgressOutput + Send + Clone + 'static,
{
    if matches.is_present(ARG_NO_PROGRESS) {
        return render_stream.boxed();
    }

    let render_stream = render_stream
        .map(|item| ProgressItem::Item(item))
        .chain(stream::once(async { ProgressItem::Complete }));

    let timer = tokio_stream::wrappers::IntervalStream::new(tokio::time::interval(
        Duration::from_millis(100),
    ))
    .map(|_| ProgressItem::Timer);

    stream::select(timer, render_stream)
        .take_while(|item| {
            let complete = match item {
                ProgressItem::Complete => true,
                _ => false,
            };
            async move { !complete }
        })
        .chain(stream::once(async { ProgressItem::Complete }))
        .map({
            let mut last_progress = None;
            move |item| match item {
                ProgressItem::Item(value) => {
                    if let Some(progress) = last_progress.clone() {
                        stream::iter(vec![
                            Ok(Box::new(ProgressSuspend) as Box<dyn Render>),
                            value,
                            Ok(Box::new(progress) as Box<dyn Render>),
                        ])
                        .boxed()
                        .right_stream()
                    } else {
                        stream::once(async { value }).boxed().right_stream()
                    }
                }
                ProgressItem::Timer => {
                    let progress = get_progress();
                    if last_progress.as_ref() != Some(&progress) {
                        last_progress = Some(progress.clone());
                        stream::once(async move { Ok(Box::new(progress) as Box<dyn Render>) })
                            .boxed()
                            .right_stream()
                    } else {
                        stream::empty().left_stream()
                    }
                }
                ProgressItem::Complete => {
                    if last_progress.is_some() {
                        last_progress = None;
                        stream::once(async { Ok(Box::new(ProgressClear) as Box<dyn Render>) })
                            .boxed()
                            .right_stream()
                    } else {
                        stream::empty().left_stream()
                    }
                }
            }
        })
        .flatten()
        .boxed()
}

impl Render for ProgressOutput {
    fn render_tty(&self, _matches: &ArgMatches, w: &mut dyn Write) -> Result<(), Error> {
        let width = textwrap::termwidth().saturating_sub(5);
        if width > 10 {
            let bar_width = width * self.value as usize / self.total as usize;
            let bar_text = format!("  {:width$}  ", self.message, width = width);
            write!(
                w,
                "\r\x1B[K\x1B[42;30m{0}\x1B[m{1}\r",
                &bar_text[..bar_width],
                &bar_text[bar_width..]
            )?;
            w.flush()?;
        }
        Ok(())
    }
}

impl Render for ProgressSuspend {
    fn render_tty(&self, _matches: &ArgMatches, w: &mut dyn Write) -> Result<(), Error> {
        write!(w, "\r\x1B[K")?;
        Ok(())
    }
}

impl Render for ProgressClear {
    fn render_tty(&self, _matches: &ArgMatches, w: &mut dyn Write) -> Result<(), Error> {
        write!(w, "\r\x1B[K")?;
        w.flush()?;
        Ok(())
    }
}
