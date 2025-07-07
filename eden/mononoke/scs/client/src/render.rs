/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Rendering of responses.
use std::io::IsTerminal;
use std::io::Write;

use anyhow::Result;
use auto_impl::auto_impl;
use futures::stream;
use futures::stream::Stream;
use futures::stream::TryStreamExt;

// Auto-impl for Box so that render streams can contain either R: Render
// or Box<dyn Render>.
#[auto_impl(Box)]
/// A renderable item.  This trait should be implemented by anything that can
/// be output from a command.
pub(crate) trait Render: Send {
    type Args;
    /// Render output suitable for human users.
    fn render(&self, _matches: &Self::Args, _write: &mut dyn Write) -> Result<()> {
        Ok(())
    }

    /// Render output suitable for human users to a terminal or console.
    fn render_tty(&self, matches: &Self::Args, write: &mut dyn Write) -> Result<()> {
        self.render(matches, write)
    }

    /// Render as a JSON value.
    fn render_json(&self, _matches: &Self::Args, _write: &mut dyn Write) -> Result<()> {
        Ok(())
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) enum OutputFormat {
    Text,
    Json,
}

trait WriteAndIsTerminal: Write + IsTerminal {}
impl WriteAndIsTerminal for std::io::Stdout {}
impl WriteAndIsTerminal for std::io::Stderr {}

impl OutputFormat {
    /// Render the output for a command invocation to stdout.
    pub(crate) async fn render<R: Render>(
        self,
        matches: &R::Args,
        objs: impl Stream<Item = Result<R>>,
    ) -> Result<()> {
        self.render_to(matches, objs, false).await
    }

    /// Render the output to stderr.
    pub(crate) async fn render_stderr<R: Render>(
        self,
        matches: &R::Args,
        objs: impl Stream<Item = Result<R>>,
    ) -> Result<()> {
        self.render_to(matches, objs, true).await
    }

    /// Render the output for a command invocation.
    pub(crate) async fn render_to<R: Render>(
        self,
        matches: &R::Args,
        objs: impl Stream<Item = Result<R>>,
        stderr: bool,
    ) -> Result<()> {
        objs.try_for_each(move |output| async move {
            let out: &mut dyn WriteAndIsTerminal = if stderr {
                &mut std::io::stderr()
            } else {
                &mut std::io::stdout()
            };
            match self {
                OutputFormat::Json => {
                    output.render_json(matches, out)?;
                    writeln!(out)?;
                }
                OutputFormat::Text => {
                    if out.is_terminal() {
                        output.render_tty(matches, out)?;
                    } else {
                        output.render(matches, out)?;
                    }
                }
            }
            Ok(())
        })
        .await
    }

    /// Render a single element for a command invocation
    pub(crate) async fn render_one<R: Render>(self, matches: &R::Args, obj: R) -> Result<()> {
        self.render(matches, stream::once(futures::future::ok(obj)))
            .await
    }
}
