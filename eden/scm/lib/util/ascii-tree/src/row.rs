/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fmt;

pub struct Row {
    pub columns: Vec<String>,
}

pub struct Rows {
    pub rows: Vec<Row>,
    pub column_alignments: Vec<Alignment>,
    pub column_min_widths: Vec<usize>,
    pub column_max_widths: Vec<usize>,
}

pub enum Alignment {
    Left,
    Right,
}

impl fmt::Display for Rows {
    /// Render rows with aligned columns.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let column_count = self.rows.iter().map(|r| r.columns.len()).max().unwrap_or(0);
        let column_widths: Vec<usize> = (0..column_count)
            .map(|i| {
                self.rows
                    .iter()
                    .map(|r| r.columns.get(i).map_or(0, |s| s.len()))
                    .max()
                    .unwrap_or(0)
                    .max(self.column_min_widths.get(i).cloned().unwrap_or(0))
                    .min(self.column_max_widths.get(i).cloned().unwrap_or(usize::MAX))
            })
            .collect();
        for row in self.rows.iter() {
            for (i, cell) in row.columns.iter().enumerate() {
                let width = column_widths[i];
                let pad = " ".repeat(width.max(cell.len()) - cell.len());
                let mut content = match self.column_alignments.get(i).unwrap_or(&Alignment::Left) {
                    Alignment::Left => cell.clone() + &pad,
                    Alignment::Right => pad + cell,
                };
                if i + 1 == row.columns.len() {
                    content = content.trim_end().to_string();
                };
                if !content.is_empty() {
                    if i != 0 {
                        // Separator
                        write!(f, " ")?;
                    }
                    write!(f, "{}", content)?;
                }
            }
            write!(f, "\n")?;
        }
        Ok(())
    }
}
