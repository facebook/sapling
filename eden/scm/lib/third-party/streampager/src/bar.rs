//! A horizontal bar on the screen.

use std::cmp::min;
use std::sync::Arc;

use termwiz::cell::CellAttributes;
use termwiz::color::AnsiColor;
use termwiz::surface::change::Change;
use termwiz::surface::Position;
use unicode_width::UnicodeWidthStr;

use crate::util;

/// A horizontal bar on the screen, e.g. the ruler or search bar.
pub(crate) struct Bar {
    left_items: Vec<Arc<dyn BarItem>>,
    right_items: Vec<Arc<dyn BarItem>>,
    style: BarStyle,
}

/// An item in a bar.
pub(crate) trait BarItem {
    fn width(&self) -> usize;
    fn render(&self, changes: &mut Vec<Change>, width: usize);
}

/// The style of the bar.  This mostly affects the default background color.
#[allow(unused)]
#[derive(Clone, Copy, Debug)]
pub(crate) enum BarStyle {
    // A normal bar with a silver background.
    Normal,

    // An informational bar with a teal background.
    Information,

    // A warning bar with a yellow background.
    Warning,

    // An error bar with a red background.
    Error,
}

impl BarStyle {
    fn background_color(self) -> AnsiColor {
        match self {
            BarStyle::Normal => AnsiColor::Silver,
            BarStyle::Information => AnsiColor::Teal,
            BarStyle::Warning => AnsiColor::Olive,
            BarStyle::Error => AnsiColor::Maroon,
        }
    }
}

impl Bar {
    pub(crate) fn new(style: BarStyle) -> Self {
        let left_items = Vec::new();
        let right_items = Vec::new();
        Bar {
            left_items,
            right_items,
            style,
        }
    }

    pub(crate) fn add_left_item(&mut self, item: Arc<dyn BarItem>) {
        self.left_items.push(item);
    }

    pub(crate) fn add_right_item(&mut self, item: Arc<dyn BarItem>) {
        self.right_items.push(item);
    }

    /// Render the bar to the given row on screen.
    pub(crate) fn render(&self, changes: &mut Vec<Change>, row: usize, width: usize) {
        changes.push(Change::CursorPosition {
            x: Position::Absolute(0),
            y: Position::Absolute(row),
        });
        let bar_attribs = CellAttributes::default()
            .set_foreground(AnsiColor::Black)
            .set_background(self.style.background_color())
            .clone();

        if width < 8 {
            // The area is too small to write anything useful, just write a blank bar.
            changes.push(Change::AllAttributes(bar_attribs));
            changes.push(Change::ClearToEndOfLine(
                self.style.background_color().into(),
            ));
            return;
        }

        let padded_item_width = |item: &Arc<dyn BarItem>| match item.width() {
            0 => 0,
            w => w + 2,
        };
        let mut left_items_width = self.left_items.iter().map(padded_item_width).sum();
        let mut right_items_width = self.right_items.iter().map(padded_item_width).sum();

        // The right-hand side is shown only if it can fit.
        if right_items_width + 2 > width {
            // Show only left items.
            right_items_width = 0;
            left_items_width = min(left_items_width, width.saturating_sub(2));
        } else {
            // Show both items, truncating or padding the left items to the remaining width.
            left_items_width = width.saturating_sub(right_items_width + 2);
        }

        changes.push(Change::AllAttributes(bar_attribs.clone()));
        changes.push(Change::Text(String::from("  ")));
        let rendered_left_width = self.render_items(
            changes,
            self.left_items.as_slice(),
            left_items_width.saturating_sub(2),
        );
        if right_items_width > 0 {
            changes.push(Change::AllAttributes(bar_attribs));
            let gap = left_items_width.saturating_sub(rendered_left_width);
            changes.push(Change::Text(" ".repeat(gap)));
            self.render_items(changes, self.right_items.as_slice(), right_items_width);
        }
        changes.push(Change::ClearToEndOfLine(
            self.style.background_color().into(),
        ));
    }

    fn render_items(
        &self,
        changes: &mut Vec<Change>,
        items: &[Arc<dyn BarItem>],
        width: usize,
    ) -> usize {
        let mut rendered_width = 0;
        for item in items.iter() {
            let item_width = item.width().min(width.saturating_sub(rendered_width));
            if item_width > 0 {
                item.render(changes, item_width);
                rendered_width += item_width;
                let pad = min(width - rendered_width, 2);
                changes.push(Change::Text(" ".repeat(pad)));
                rendered_width += pad;
                if rendered_width >= width {
                    break;
                }
            }
        }
        rendered_width
    }
}

pub(crate) struct BarString(String);

impl BarString {
    pub(crate) fn new(s: impl Into<String>) -> Self {
        BarString(s.into())
    }
}

impl BarItem for BarString {
    fn width(&self) -> usize {
        self.0.as_str().width()
    }

    fn render(&self, changes: &mut Vec<Change>, width: usize) {
        changes.push(Change::Text(util::truncate_string(
            self.0.as_str(),
            0,
            width,
        )));
    }
}
