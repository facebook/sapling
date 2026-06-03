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

pub(crate) struct OutputRendererOptions {
    pub(crate) min_row_height: usize,
}

#[derive(Default)]
pub(crate) struct OutputRendererState {
    pub(crate) extra_pad_line: Option<String>,
}

impl OutputRendererState {
    pub(crate) fn push_line(&mut self, out: &mut String, line: &str) {
        out.push_str(line.trim_end());
        out.push('\n');
    }

    pub(crate) fn mabye_push_blank_line(&mut self, out: &mut String) {
        out.push('\n');
    }
}

pub struct OutputRendererBuilder<N, R>
where
    R: Renderer<N, Output = GraphRow<N>> + Sized,
{
    inner: R,
    options: OutputRendererOptions,
    _phantom: PhantomData<N>,
}

impl<N, R> OutputRendererBuilder<N, R>
where
    R: Renderer<N, Output = GraphRow<N>> + Sized,
{
    pub fn new(inner: R) -> Self {
        OutputRendererBuilder {
            inner,
            options: OutputRendererOptions {
                min_row_height: DEFAULT_MIN_ROW_HEIGHT,
            },
            _phantom: PhantomData,
        }
    }

    pub fn with_min_row_height(mut self, min_row_height: usize) -> Self {
        self.options.min_row_height = min_row_height;
        self.inner.set_min_row_height(min_row_height);
        self
    }

    pub fn with_stagger_consecutive_disconnected_nodes(mut self, stagger: bool) -> Self {
        self.inner.set_stagger_disconnected_nodes(stagger);
        self
    }

    pub fn build_ascii(self) -> AsciiRenderer<N, R> {
        AsciiRenderer::new(self.inner, self.options)
    }

    pub fn build_ascii_large(self) -> AsciiLargeRenderer<N, R> {
        AsciiLargeRenderer::new(self.inner, self.options)
    }

    pub fn build_box_drawing(self) -> BoxDrawingRenderer<N, R> {
        BoxDrawingRenderer::new(self.inner, self.options)
    }
}
