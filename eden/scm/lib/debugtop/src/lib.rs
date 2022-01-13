/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::Error;
use runlog::{Entry, Progress};

pub struct TableGenerator {
    column_titles: Vec<String>,
    row_generator: Vec<fn(&Entry) -> String>,
}

impl TableGenerator {
    pub fn new(column_titles_str: String) -> Result<TableGenerator, Vec<String>> {
        let column_funcs: Vec<(&str, fn(&Entry) -> String)> = vec![
            ("PID", |entry| entry.pid.to_string()),
            ("PROGRESS", |entry| top_progress_entry(&entry.progress)),
            ("TIME SPENT", |entry| {
                let time_spent = chrono::offset::Utc::now() - entry.start_time;
                top_time_entry(time_spent)
            }),
            ("CMD", |entry| entry.command.join(" ")),
        ];
        let columns_map: HashMap<_, _> = column_funcs.iter().copied().collect();
        let column_titles: Vec<_> = if !column_titles_str.is_empty() {
            column_titles_str.split(',').map(|x| x.trim()).collect()
        } else {
            column_funcs.iter().map(|(x, _)| *x).collect()
        };

        let unexpected_columns: Vec<_> = column_titles
            .iter()
            .filter(|&c| !columns_map.contains_key(c))
            .copied()
            .collect();
        if !unexpected_columns.is_empty() {
            return Err(unexpected_columns
                .iter()
                .map(|&x| String::from(x))
                .collect());
        }
        Ok(TableGenerator {
            column_titles: column_titles.iter().map(|&x| String::from(x)).collect(),
            row_generator: column_titles
                .iter()
                .map(|&c| *columns_map.get(c).unwrap())
                .collect(),
        })
    }

    pub fn column_titles(&self) -> &Vec<String> {
        &self.column_titles
    }

    pub fn generate_rows<'a>(
        &'a self,
        runlog_entries: impl Iterator<Item = Result<(Entry, bool), Error>> + 'a,
    ) -> impl Iterator<Item = Vec<String>> + 'a {
        runlog_entries.filter_map(|entry| {
            let (entry, running) = match entry {
                Ok((entry, running)) => (entry, running),
                Err(_) => {
                    return None;
                }
            };
            if !running {
                return None;
            }
            Some(self.row_generator.iter().map(|&f| f(&entry)).collect())
        })
    }
}

fn top_time_entry(time_spent: chrono::Duration) -> String {
    static MILLIS_IN_SECOND: i64 = 1000;
    static MILLIS_IN_MINUTE: i64 = MILLIS_IN_SECOND * 60;
    static MILLIS_IN_HOUR: i64 = MILLIS_IN_MINUTE * 60;
    let milliseconds_spent = time_spent.num_milliseconds();
    let (unit_str, millis_in_unit) = if milliseconds_spent >= MILLIS_IN_HOUR {
        ("h", MILLIS_IN_HOUR)
    } else if milliseconds_spent >= MILLIS_IN_MINUTE {
        ("m", MILLIS_IN_MINUTE)
    } else {
        ("s", MILLIS_IN_SECOND)
    };
    let units_spent = milliseconds_spent as f64 / millis_in_unit as f64;
    if units_spent < 10_f64 {
        format!("{:.1}{}", units_spent, unit_str)
    } else {
        format!("{:.0}{}", units_spent, unit_str)
    }
}

fn top_progress_entry(progress_bars: &[Progress]) -> String {
    for progress_bar in progress_bars.iter() {
        if progress_bar.total > 0 {
            return format!(
                "{:.1}%",
                progress_bar.position as f32 / progress_bar.total as f32 * 100_f32
            );
        }
    }
    String::from("-")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_time_entry() {
        assert_eq!(
            top_time_entry(chrono::Duration::hours(5) + chrono::Duration::minutes(6)),
            "5.1h"
        );
        assert_eq!(
            top_time_entry(chrono::Duration::hours(12) + chrono::Duration::minutes(6)),
            "12h"
        );
        assert_eq!(
            top_time_entry(chrono::Duration::minutes(2) + chrono::Duration::seconds(42)),
            "2.7m"
        );
        assert_eq!(
            top_time_entry(chrono::Duration::minutes(30) + chrono::Duration::seconds(42)),
            "31m"
        );
        assert_eq!(
            top_time_entry(chrono::Duration::seconds(1) + chrono::Duration::milliseconds(357)),
            "1.4s"
        );
        assert_eq!(
            top_time_entry(chrono::Duration::seconds(41) + chrono::Duration::milliseconds(357)),
            "41s"
        );
    }

    #[test]
    fn test_progress_entry() {
        // Test picking only the first progress bar that has a positive total
        assert_eq!(
            top_progress_entry(&[
                Progress {
                    topic: String::from(""),
                    unit: String::from(""),
                    position: 0,
                    total: 0,
                },
                Progress {
                    topic: String::from(""),
                    unit: String::from(""),
                    position: 123,
                    total: 1000,
                },
                Progress {
                    topic: String::from(""),
                    unit: String::from(""),
                    position: 0,
                    total: 100000,
                },
            ]),
            "12.3%"
        );
        // Test total = 0
        assert_eq!(
            top_progress_entry(&[Progress {
                topic: String::from(""),
                unit: String::from(""),
                position: 0,
                total: 0,
            },]),
            "-"
        );
        // Test empty list
        assert_eq!(top_progress_entry(&[]), "-");
    }
}
