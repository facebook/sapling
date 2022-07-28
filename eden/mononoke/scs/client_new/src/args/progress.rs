/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Adapters for rendering progress

use std::time::Duration;

use crate::render::Render;
use anyhow::Result;
use futures::stream;
use futures::stream::Stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use std::io::Write;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProgressOutput {
    message: String,
    value: u64,
    total: u64,
}

#[derive(clap::Parser)]
pub(crate) struct ProgressArgs {
    #[clap(long)]
    /// Don't show the progress bar
    no_progress: bool,
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

enum ProgressItem<I: Render<Args = ()>> {
    Item(Result<I>),
    Complete,
    Timer,
}

pub(crate) enum ProgressRender<I: Render<Args = ()>> {
    Item(I),
    Suspend,
    Clear,
    Output(ProgressOutput),
}

impl ProgressArgs {
    pub(crate) fn render_progress<I: Render<Args = ()> + 'static>(
        &self,
        render_stream: impl Stream<Item = Result<I>> + Send + 'static,
        get_progress: impl Fn() -> ProgressOutput + Send + Clone + 'static,
    ) -> impl Stream<Item = Result<ProgressRender<I>>> + Send + 'static {
        if self.no_progress {
            return render_stream.map_ok(ProgressRender::Item).left_stream();
        }

        let render_stream = render_stream
            .map(ProgressItem::Item)
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
                                Ok(ProgressRender::Suspend),
                                value.map(ProgressRender::Item),
                                Ok(ProgressRender::Output(progress)),
                            ])
                            .boxed()
                        } else {
                            stream::once(async { value.map(ProgressRender::Item) }).boxed()
                        }
                    }
                    ProgressItem::Timer => {
                        let progress = get_progress();
                        if last_progress.as_ref() != Some(&progress) {
                            last_progress = Some(progress.clone());
                            stream::once(async move { Ok(ProgressRender::Output(progress)) })
                                .boxed()
                        } else {
                            stream::empty().boxed()
                        }
                    }
                    ProgressItem::Complete => {
                        if last_progress.is_some() {
                            last_progress = None;
                            stream::once(async { Ok(ProgressRender::Clear) }).boxed()
                        } else {
                            stream::empty().boxed()
                        }
                    }
                }
            })
            .flatten()
            .right_stream()
    }
}

impl<I: Render<Args = ()>> Render for ProgressRender<I> {
    type Args = ();

    fn render_tty(&self, args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        match self {
            Self::Item(i) => i.render_tty(args, w)?,
            Self::Output(out) => {
                let width = textwrap::termwidth().saturating_sub(5);
                if width > 10 {
                    let bar_width = width * out.value as usize / out.total as usize;
                    let bar_text = format!("  {:width$}  ", out.message, width = width);
                    write!(
                        w,
                        "\r\x1B[K\x1B[42;30m{0}\x1B[m{1}\r",
                        &bar_text[..bar_width],
                        &bar_text[bar_width..]
                    )?;
                    w.flush()?;
                }
            }
            Self::Suspend => write!(w, "\r\x1B[K")?,
            Self::Clear => {
                write!(w, "\r\x1B[K")?;
                w.flush()?;
            }
        };
        Ok(())
    }
}
