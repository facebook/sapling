/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::Error;
#[cfg(test)]
use chrono::TimeZone;
use chrono::Utc;
use runlog::Entry;
use runlog::Progress;

struct EntryState {
    last_time: chrono::DateTime<chrono::Utc>,
    last_download_bytes: usize,
    last_upload_bytes: usize,
}

pub struct TableGenerator {
    column_titles: Vec<String>,
    row_generator: Vec<fn(&Entry, fn() -> chrono::DateTime<Utc>, Option<&EntryState>) -> String>,
    entry_removal_delay: chrono::Duration,
    entry_states: HashMap<String, EntryState>,
}

impl TableGenerator {
    pub fn new(
        column_titles_str: String,
        entry_removal_delay: chrono::Duration,
    ) -> Result<TableGenerator, Vec<String>> {
        let column_funcs: Vec<(
            &str,
            fn(&Entry, fn() -> chrono::DateTime<Utc>, Option<&EntryState>) -> String,
        )> = vec![
            ("PID", |entry, _, _| entry.pid.to_string()),
            ("STATUS", |entry, _, _| top_status_entry(&entry.exit_code)),
            ("PROGRESS", |entry, _, _| {
                top_progress_entry(&entry.progress)
            }),
            ("TIME SPENT", |entry, current_time, _| {
                let time_spent = if let Some(end_time) = entry.end_time {
                    end_time
                } else {
                    current_time()
                } - entry.start_time;
                top_time_entry(time_spent)
            }),
            ("NET DOWN", |entry, current_time, entry_state| {
                let (duration, bytes_transferred) = match entry_state {
                    Some(entry_state) => (
                        current_time() - entry_state.last_time,
                        entry.download_bytes - entry_state.last_download_bytes,
                    ),
                    None => (chrono::Duration::seconds(0), 0_usize),
                };
                network_entry(duration, bytes_transferred)
            }),
            ("NET UP", |entry, current_time, entry_state| {
                let (duration, bytes_transferred) = match entry_state {
                    Some(entry_state) => (
                        current_time() - entry_state.last_time,
                        entry.upload_bytes - entry_state.last_upload_bytes,
                    ),
                    None => (chrono::Duration::seconds(0), 0_usize),
                };
                network_entry(duration, bytes_transferred)
            }),
            ("CMD", |entry, _, _| entry.command.join(" ")),
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
            entry_removal_delay,
            entry_states: HashMap::new(),
        })
    }

    pub fn column_titles(&self) -> &Vec<String> {
        &self.column_titles
    }

    pub fn generate_rows<'a>(
        &'a mut self,
        runlog_entries: impl Iterator<Item = Result<(Entry, bool), Error>> + 'a,
        current_time: fn() -> chrono::DateTime<Utc>,
    ) -> impl Iterator<Item = Vec<String>> + 'a {
        runlog_entries.filter_map(move |entry| {
            let (entry, running) = match entry {
                Ok((entry, running)) => (entry, running),
                Err(_) => {
                    return None;
                }
            };
            let end_time_filter_fn =
                |end_time| current_time() - end_time <= self.entry_removal_delay;
            if running || entry.end_time.map_or(false, end_time_filter_fn) {
                let entry_state = self.entry_states.get(&entry.id);
                let row = self
                    .row_generator
                    .iter()
                    .map(|&f| f(&entry, current_time, entry_state))
                    .collect();
                self.entry_states.insert(
                    entry.id,
                    EntryState {
                        last_time: current_time(),
                        last_download_bytes: entry.download_bytes,
                        last_upload_bytes: entry.upload_bytes,
                    },
                );
                Some(row)
            } else {
                None
            }
        })
    }
}

fn top_status_entry(exit_code: &Option<i32>) -> String {
    match exit_code {
        Some(code) => format!("EXITED ({})", code),
        None => "RUNNING".to_string(),
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

fn network_entry(time_spent: chrono::Duration, bytes_transferred: usize) -> String {
    let time_spent = time_spent.num_seconds();
    if time_spent == 0 {
        return String::from("-");
    }
    let rate = ((bytes_transferred * 8) as f32) / (time_spent as f32);
    let (rate, prefix) = if rate >= 1e5 {
        (rate / 1e6, "M")
    } else {
        (rate / 1e3, "k")
    };
    format!("{:.1} {}b/s", rate, prefix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct ParseError {}

    impl std::fmt::Display for ParseError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "Oh no, something bad went down")
        }
    }

    impl std::error::Error for ParseError {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            None
        }
    }

    #[test]
    fn test_table_generator() {
        let default_removal_delay = chrono::Duration::seconds(1);
        let default_generator = || TableGenerator::new("".to_string(), default_removal_delay);
        let default_time = || Utc.timestamp_opt(3, 100000000).unwrap();

        // Test invalid columns
        assert!(
            matches!(TableGenerator::new("not a valid column, PID, not_valid_either".to_string(), default_removal_delay),
            Err(x) if x == vec![
                "not a valid column".to_string(),
                "not_valid_either".to_string()
            ])
        );
        // Test default column order
        let default_columns = vec![
            "PID".to_string(),
            "STATUS".to_string(),
            "PROGRESS".to_string(),
            "TIME SPENT".to_string(),
            "NET DOWN".to_string(),
            "NET UP".to_string(),
            "CMD".to_string(),
        ];
        assert_eq!(
            *default_generator().unwrap().column_titles(),
            default_columns
        );
        // Test specific columns
        assert_eq!(
            *TableGenerator::new("CMD,   PID".to_string(), default_removal_delay)
                .unwrap()
                .column_titles(),
            vec!["CMD".to_string(), "PID".to_string()]
        );
        // Test single row column format
        let runlog_entries = vec![Ok((
            Entry {
                id: "1".to_string(),
                command: vec!["somecommand".to_string(), "somearg".to_string()],
                pid: 101,
                download_bytes: 0,
                upload_bytes: 0,
                start_time: Utc.timestamp_opt(0, 0).unwrap(),
                end_time: None,
                exit_code: None,
                progress: vec![Progress {
                    topic: "spinning".to_string(),
                    unit: "".to_string(),
                    total: 100,
                    position: 2,
                }],
            },
            true,
        ))];
        let expected_rows = vec![vec![
            "101".to_string(),
            "RUNNING".to_string(),
            "2.0%".to_string(),
            "3.1s".to_string(),
            "-".to_string(),
            "-".to_string(),
            "somecommand somearg".to_string(),
        ]];
        assert_eq!(
            default_generator()
                .unwrap()
                .generate_rows(runlog_entries.into_iter(), default_time)
                .collect::<Vec<_>>(),
            expected_rows
        );
        // Test row filtering
        let err = ParseError {};
        let runlog_entries: Vec<Result<(Entry, bool), Error>> = vec![
            Err(Error::new(err)),
            Ok((
                Entry {
                    id: "0".to_string(),
                    command: vec!["".to_string()],
                    pid: 0,
                    download_bytes: 0,
                    upload_bytes: 0,
                    start_time: Utc.timestamp_opt(0, 0).unwrap(),
                    end_time: Some(Utc.timestamp_opt(2, 0).unwrap()),
                    exit_code: Some(0),
                    progress: vec![],
                },
                false,
            )),
            Ok((
                Entry {
                    id: "1".to_string(),
                    command: vec!["notarealcommand".to_string()],
                    pid: 321,
                    download_bytes: 0,
                    upload_bytes: 0,
                    start_time: Utc.timestamp_opt(0, 0).unwrap(),
                    end_time: Some(Utc.timestamp_opt(3, 0).unwrap()),
                    exit_code: Some(123),
                    progress: vec![],
                },
                false,
            )),
        ];

        let expected_rows = vec![vec![
            "321".to_string(),
            "EXITED (123)".to_string(),
            "-".to_string(),
            "3.0s".to_string(),
            "-".to_string(),
            "-".to_string(),
            "notarealcommand".to_string(),
        ]];
        assert_eq!(
            default_generator()
                .unwrap()
                .generate_rows(runlog_entries.into_iter(), default_time)
                .collect::<Vec<_>>(),
            expected_rows
        );
    }

    #[test]
    fn test_status_entry() {
        assert_eq!(top_status_entry(&Some(0)), "EXITED (0)");
        assert_eq!(top_status_entry(&None), "RUNNING");
    }

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

    #[test]
    fn test_network_entry() {
        assert_eq!(
            network_entry(chrono::Duration::seconds(0), 5000),
            String::from("-"),
        );
        assert_eq!(
            network_entry(chrono::Duration::seconds(2), 400),
            String::from("1.6 kb/s"),
        );
        assert_eq!(
            network_entry(chrono::Duration::seconds(1), 37500),
            String::from("0.3 Mb/s"),
        );
    }
}
