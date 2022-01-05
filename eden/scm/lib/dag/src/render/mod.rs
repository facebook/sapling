/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod ascii;
mod ascii_large;
mod box_drawing;
mod column;
mod output;
#[allow(clippy::module_inception)]
mod render;
mod render_utils;

#[cfg(test)]
mod test_fixtures;

#[cfg(test)]
mod test_utils;

pub use self::ascii::AsciiRenderer;
pub use self::ascii_large::AsciiLargeRenderer;
pub use self::box_drawing::BoxDrawingRenderer;
pub use self::render::Ancestor;
pub use self::render::GraphRowRenderer;
pub use self::render::LinkLine;
pub use self::render::NodeLine;
pub use self::render::PadLine;
pub use self::render::Renderer;
pub use self::render_utils::render_namedag;
#[cfg(any(test, feature = "indexedlog-backend"))]
pub use self::render_utils::render_segment_dag;
