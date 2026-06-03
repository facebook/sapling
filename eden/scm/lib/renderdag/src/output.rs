/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::marker::PhantomData;

use super::ascii::AsciiRenderer;
use super::ascii_large::AsciiLargeRenderer;
use super::box_drawing::BoxDrawingRenderer;
use super::render::GraphRow;
use super::render::Renderer;

pub(crate) const DEFAULT_MIN_ROW_HEIGHT: usize = 2;

#[derive(Clone, Copy, Debug)]
pub struct OutputRendererOptions {
    pub min_row_height: usize,
    pub stagger_consecutive_disconnected_nodes: bool,
}

/// Common stateful string line output utilities shared by ASCII and box-drawing
/// renderers.
#[derive(Default)]
pub(crate) struct OutputRendererState {
    queued_pad_line: Option<String>,
    last_line_is_blank: bool,
}

impl OutputRendererState {
    pub(crate) fn begin_row(&mut self, out: &mut String, separator_line: bool) {
        self.flush_queued_pad_line(out);
        if separator_line {
            self.maybe_push_separator_line(out);
        }
    }

    pub(crate) fn push_line_with_message(
        &mut self,
        out: &mut String,
        mut line: String,
        message: Option<&str>,
    ) {
        if let Some(message) = message {
            line.push(' ');
            line.push_str(message);
        }
        self.push_line(out, &line);
    }

    pub(crate) fn push_pad_lines<'a>(
        &mut self,
        out: &mut String,
        base_pad_line: &str,
        message_lines: impl Iterator<Item = &'a str>,
    ) -> bool {
        let mut emitted = false;
        for message in message_lines {
            let mut pad_line = String::with_capacity(base_pad_line.len() + message.len() + 1);
            pad_line.push_str(base_pad_line);
            pad_line.push(' ');
            pad_line.push_str(message);
            self.push_line(out, &pad_line);
            emitted = true;
        }
        emitted
    }

    pub(crate) fn queue_pad_line(&mut self, pad_line: String) {
        self.queued_pad_line = Some(pad_line);
    }

    fn push_line(&mut self, out: &mut String, line: &str) {
        out.push_str(line.trim_end());
        out.push('\n');
        self.last_line_is_blank = line.trim_end().is_empty();
    }

    fn maybe_push_separator_line(&mut self, out: &mut String) {
        if !self.last_line_is_blank {
            out.push('\n');
            self.last_line_is_blank = true;
        }
    }

    fn flush_queued_pad_line(&mut self, out: &mut String) {
        if let Some(pad_line) = self.queued_pad_line.take() {
            self.push_line(out, &pad_line);
        }
    }
}

pub struct OutputRendererBuilder<N, R>
where
    R: Renderer<N, Output = GraphRow<N>> + Sized,
{
    inner: R,
    _phantom: PhantomData<N>,
}

impl Default for OutputRendererOptions {
    fn default() -> Self {
        Self {
            min_row_height: DEFAULT_MIN_ROW_HEIGHT,
            stagger_consecutive_disconnected_nodes: false,
        }
    }
}

impl<N, R> OutputRendererBuilder<N, R>
where
    R: Renderer<N, Output = GraphRow<N>> + Sized,
{
    pub fn new(inner: R) -> Self {
        OutputRendererBuilder {
            inner,
            _phantom: PhantomData,
        }
    }

    pub fn with_options(mut self, options: OutputRendererOptions) -> Self {
        *self.inner.output_options_mut() = options;
        self
    }

    pub fn with_min_row_height(mut self, min_row_height: usize) -> Self {
        self.inner.output_options_mut().min_row_height = min_row_height;
        self
    }

    pub fn with_stagger_consecutive_disconnected_nodes(mut self, stagger: bool) -> Self {
        self.inner
            .output_options_mut()
            .stagger_consecutive_disconnected_nodes = stagger;
        self
    }

    pub fn build_ascii(self) -> AsciiRenderer<N, R> {
        AsciiRenderer::new(self.inner)
    }

    pub fn build_ascii_large(self) -> AsciiLargeRenderer<N, R> {
        AsciiLargeRenderer::new(self.inner)
    }

    pub fn build_box_drawing(self) -> BoxDrawingRenderer<N, R> {
        BoxDrawingRenderer::new(self.inner)
    }
}
