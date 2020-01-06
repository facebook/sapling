/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::marker::PhantomData;

use crate::ascii::AsciiRenderer;
use crate::ascii_large::AsciiLargeRenderer;
use crate::box_drawing::BoxDrawingRenderer;
use crate::render::{GraphRow, Renderer};

pub(crate) struct OutputRendererOptions {
    pub(crate) min_row_height: usize,
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
            options: OutputRendererOptions { min_row_height: 2 },
            _phantom: PhantomData,
        }
    }

    pub fn with_min_row_height(mut self, min_row_height: usize) -> Self {
        self.options.min_row_height = min_row_height;
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
