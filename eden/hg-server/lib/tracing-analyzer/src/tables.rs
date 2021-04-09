/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Analyze tracing data for edenscm
//!
//! This is edenscm application specific. It's not a general purposed library.

use serde_json::Value;
// use std::borrow::Cow;
use std::collections::BTreeMap as Map;
use tracing_collector::model::{IndexMap, TreeSpan, TreeSpans};

type Row = Map<String, Value>;
type Rows = Vec<Row>;
type Tables = Map<String, Rows>;
type TidSpans<'a> = IndexMap<(u64, u64), TreeSpans<&'a str>>;

// TODO: Make things more configurable.

/// Extract rows from tracing data. Output format is similar to NoSQL tables:
///
/// ```plain,ignore
/// {table_name: [{column_name: column_data}]}
/// ```
pub fn extract_tables(tid_spans: &TidSpans) -> Tables {
    let mut tables = Map::new();
    extract_dev_command_timers(&mut tables, tid_spans);
    extract_other_tables(&mut tables, tid_spans);
    tables
}

fn extract_dev_command_timers<'a>(tables: &mut Tables, tid_spans: &TidSpans) {
    let mut row = Row::new();
    let toint = |value: &str| -> Value { value.parse::<i64>().unwrap_or_default().into() };

    for spans in tid_spans.values() {
        for span in spans.walk() {
            match span.meta.get("name").cloned().unwrap_or("") {
                // By hgcommands, run.rs
                "Run Command" => {
                    let duration = span.duration_millis().unwrap_or(0);
                    row.insert("command_duration".into(), duration.into());
                    row.insert("elapsed".into(), duration.into());

                    for (&name, &value) in span.meta.iter() {
                        match name {
                            "nice" => {
                                row.insert("nice".into(), toint(value));
                            }
                            "version" => {
                                // Truncate the "version" string. This matches the old telemetry behavior.
                                row.insert("version".into(), value[..34.min(value.len())].into());
                            }
                            "max_rss" => {
                                row.insert("maxrss".into(), toint(value));
                            }
                            "exit_code" => {
                                row.insert("errorcode".into(), toint(value));
                            }
                            "parent_names" => {
                                if let Ok(names) = serde_json::from_str::<Vec<String>>(value) {
                                    let name = names.get(0).cloned().unwrap_or_default();
                                    row.insert("parent".into(), name.into());
                                }
                            }
                            "args" => {
                                if let Ok(args) = serde_json::from_str::<Vec<String>>(value) {
                                    // Normalize the first argument to "hg".
                                    let mut full = "hg".to_string();
                                    for arg in args.into_iter().skip(1) {
                                        // Keep the length bounded.
                                        if full.len() + arg.len() >= 256 {
                                            full += " (truncated)";
                                            break;
                                        }
                                        full += &" ";
                                        // TODO: Use shell_escape once in tp2.
                                        // full += &shell_escape::unix::escape(Cow::Owned(arg));
                                        full += &arg;
                                    }
                                    row.insert("fullcommand".into(), full.into());
                                }
                            }
                            _ => {}
                        }
                    }
                }

                // The "log:command-row" event is used by code that wants to
                // log to columns of the main command row easily.
                "log:command-row" if span.is_event => {
                    extract_span(&span, &mut row);
                }

                _ => {}
            }
        }
    }

    tables.insert("dev_command_timers".into(), vec![row]);
}

fn extract_other_tables<'a>(tables: &mut Tables, tid_spans: &TidSpans) {
    for spans in tid_spans.values() {
        for span in spans.walk() {
            match span.meta.get("name").cloned().unwrap_or("") {
                // The "log:create-row" event is used by code that wants to log
                // to a entire new column in a specified table.
                //
                // The event is expected to have "table", and the rest of the
                // metadata will be logged as-is.
                "log:create-row" => {
                    let table_name = match span.meta.get("table") {
                        Some(&name) => name,
                        None => continue,
                    };
                    let mut row = Row::new();
                    extract_span(span, &mut row);
                    tables.entry(table_name.into()).or_default().push(row);
                }
                _ => {}
            }
        }
    }
}

/// Parse a span, extract its metadata to a row.
fn extract_span(span: &TreeSpan<&str>, row: &mut Row) {
    for (&name, &value) in span.meta.iter() {
        match name {
            // Those keys are likely generated. Skip them.
            "module_path" | "cat" | "line" | "name" => {}

            // Attempt to convert it to an integer (since tracing data is
            // string only).
            _ => match value.parse::<i64>() {
                Ok(i) => {
                    row.insert(name.into(), i.into());
                }
                _ => {
                    row.insert(name.into(), value.into());
                }
            },
        }
    }
}
