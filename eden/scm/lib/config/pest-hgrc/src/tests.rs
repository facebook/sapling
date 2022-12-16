/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::parse;

#[test]
fn test_parse_basic() {
    let config = r#"
# comment
[section1]
name1=value1
name2 = value2

[section2]
name3 = mutliple line,
  line 2
  line 3
    line 4

%unset name3
%include bar
"#;
    assert_eq!(
        format!("{:#?}", parse(config).unwrap()),
        r#"[
    SetConfig {
        section: "section1",
        name: "name1",
        value: "value1",
        span: 28..34,
    },
    SetConfig {
        section: "section1",
        name: "name2",
        value: "value2",
        span: 43..49,
    },
    SetConfig {
        section: "section2",
        name: "name3",
        value: "mutliple line,\nline 2\nline 3\nline 4",
        span: 70..113,
    },
    UnsetConfig {
        section: "section2",
        name: "name3",
        span: 122..127,
    },
    Include {
        path: "bar",
        span: 137..140,
    },
]"#
    );
}

#[test]
fn test_parse_error() {
    let config = "%set a b";
    assert_eq!(
        format!("{}", parse(config).unwrap_err()),
        "line 1: unknown directive (expect '%include' or '%unset')"
    );
}
