/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Rendering of responses.
use std::io::Write;

use anyhow::Error;
use clap::ArgMatches;
use futures::stream::BoxStream;

/// A renderable item.  This trait should be implemented by anything that can
/// be output from a command.
pub(crate) trait Render: Send {
    /// Render output suitable for human users.
    fn render(&self, _matches: &ArgMatches, _write: &mut dyn Write) -> Result<(), Error> {
        Ok(())
    }

    /// Render output suitable for human users to a terminal or console.
    fn render_tty(&self, matches: &ArgMatches, write: &mut dyn Write) -> Result<(), Error> {
        self.render(matches, write)
    }

    /// Render as a JSON value.
    fn render_json(&self, _matches: &ArgMatches, _write: &mut dyn Write) -> Result<(), Error> {
        Ok(())
    }
}

pub(crate) type RenderStream = BoxStream<'static, Result<Box<dyn Render>, Error>>;
