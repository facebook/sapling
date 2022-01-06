/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::io::Write;
use std::thread::sleep;
use std::time::Duration;

use chrono;
use clidispatch::io::IsTty;
use cliparser::define_flags;
use comfy_table::Table;
use runlog::{Entry, Progress};

use super::Repo;
use super::Result;
use super::IO;

define_flags! {
    pub struct DebugTopOpts {
        /// rate to refersh in milliseconds
        #[short('r')]
        refresh_rate: i64 = 1000,

        /// columns separated by comma; shows all if none is specified
        #[short('c')]
        columns: String = "",
    }
}

pub fn run(opts: DebugTopOpts, io: &IO, repo: Repo) -> Result<u8> {
    let mut stdout = io.output();
    let mut stderr = io.error();
    let running_in_tty = stdout.is_tty();
    let refresh_rate = opts.refresh_rate.max(0) as u64;
    let column_funcs: Vec<(&str, &dyn Fn(&Entry) -> String)> = vec![
        ("PID", &|entry| entry.pid.to_string()),
        ("PROGRESS", &|entry| top_progress_entry(&entry.progress)),
        ("TIME SPENT", &|entry| {
            let time_spent = chrono::offset::Utc::now() - entry.start_time;
            top_time_entry(time_spent)
        }),
        ("CMD", &|entry| entry.command.join(" ")),
    ];
    let columns_map: HashMap<_, _> = column_funcs.iter().copied().collect();
    let column_titles: Vec<_> = if !opts.columns.is_empty() {
        opts.columns.split(',').map(|x| x.trim()).collect()
    } else {
        column_funcs.iter().map(|(x, _)| *x).collect()
    };

    let unexpected_columns: Vec<_> = column_titles
        .iter()
        .filter(|&c| !columns_map.contains_key(c))
        .copied()
        .collect();
    if !unexpected_columns.is_empty() {
        for column in unexpected_columns.iter() {
            write!(stderr, "Error: column \"{}\" was not expected\n", column)?;
        }
        return Ok(22);
    }

    let row_generator: Vec<&dyn Fn(&Entry) -> String> = column_titles
        .iter()
        .map(|&c| *columns_map.get(c).unwrap())
        .collect();

    loop {
        let mut table = Table::new();
        table.set_header(&column_titles);
        for entry in runlog::FileStore::entry_iter(repo.shared_dot_hg_path().join("runlog"))? {
            let (entry, running) = match entry {
                Ok((entry, running)) => (entry, running),
                Err(_) => {
                    continue;
                }
            };
            if !running {
                continue;
            }
            let row: Vec<_> = row_generator.iter().map(|&f| f(&entry)).collect();
            table.add_row(row);
        }
        if !running_in_tty {
            write!(stdout, "{}\n", table)?;
            break;
        }
        io.set_progress(format!("{}\n", table).as_str())?;
        sleep(Duration::from_millis(refresh_rate));
    }

    Ok(0)
}

pub fn name() -> &'static str {
    "debugtop"
}

pub fn doc() -> &'static str {
    "outputs information about all running commands for the current repository"
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
