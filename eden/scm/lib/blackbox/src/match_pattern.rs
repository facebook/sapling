/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Pattern matching for JSON value.

use serde_json::Value;
use std::collections::HashMap;

/// Test if a JSON value matches the given pattern.
///
/// Examples:
///
/// ```
/// use blackbox::match_pattern;
/// use serde_json::json;
///
/// // Value matches if they are equal. No magic convertion between types.
/// assert!(match_pattern(&json!(true), &json!(true)));
/// assert!(match_pattern(&json!(42), &json!(42)));
/// assert!(!match_pattern(&json!(true), &json!("true")));
/// assert!(!match_pattern(&json!("true"), &json!(true)));
///
/// // Object matches if the pattern is a subset of the target object.
/// assert!(match_pattern(&json!({"a": 1, "b": 2}), &json!({"b": 2})));
/// assert!(!match_pattern(&json!({"a": 1}), &json!({"b": 2, "a": 1})));
///
/// // "_" matches anything.
/// assert!(match_pattern(&json!({"a": ["b"]}), &json!({"a": "_"})));
///
/// // Array is handled specially. Its first element is always treated
/// // as an operator.
///
/// // ["or", ...]: matches if any item in the rest of the array matches.
/// assert!(match_pattern(&json!(42), &json!(["or", 1, 42, 98])));
///
/// // ["not", pattern]: negates a match.
/// assert!(match_pattern(&json!(42), &json!(["not", 43])));
///
/// // ["and", ...]: matches if every item match.
/// assert!(match_pattern(
///     &json!({"a": 1, "b": 2}),
///     &json!(["and", {"a": 1}, ["not", {"b": 3}]])));
///
/// // ["range", start, end]: matches a number x, if start <= x <= end.
/// assert!(match_pattern(&json!(42), &json!(["range", 1, 100])));
///
/// // ["prefix", ...]: matches an array if the prefix matches.
/// assert!(match_pattern(&json!(["a", "b", "c", "d"]), &json!(["prefix", "a", "_", "c"])));
///
/// // ["contain", pattern]: matches an array if one item matches.
/// assert!(match_pattern(&json!([1, 10, 50, 100]), &json!(["contain", 50])));
/// ```
pub fn match_pattern(value: &Value, pattern: &Value) -> bool {
    let mut capture = Default::default();
    match_pattern_captured(value, pattern, &mut capture)
}

/// Similar to `match_pattern`, but also support capturing matches into `capture`.
/// To capture a value, use `["capture", name, pattern]`. The captured value will
/// be stored in the returned [`HashMap`].
///
/// Examples:
///
/// ```
/// use blackbox::capture_pattern;
/// use serde_json::json;
///
/// // Capture nested objects.
/// assert_eq!(capture_pattern(
///     &json!({"a":{"b": 3}}),
///     &json!({"a":{"b":["capture", "B", "_"]}})).unwrap()["B"],
///     &json!(3));
///
/// // Capture with conditions.
/// assert_eq!(capture_pattern(
///     &json!(50),
///     &json!(["capture", "INT", ["range", 0, 100]])).unwrap()["INT"],
///     &json!(50));
///
/// // Logical expression.
/// let obj = json!(["a", "b"]);
/// let pat = json!(["or", ["capture", "INT", ["range", 0, 100]],
///                        ["capture", "LIST", ["contain", "b"]]]);
/// let captured = capture_pattern(&obj, &pat).unwrap();
/// assert_eq!(captured["LIST"], &json!(["a", "b"]));
/// assert!(!captured.contains_key("INT"));
///
/// // Not matched.
/// assert!(capture_pattern(&json!("c"), &pat).is_none());
/// ```
pub fn capture_pattern<'a, 'b>(value: &'a Value, pattern: &'b Value) -> Option<Capture<'b, 'a>> {
    let mut capture = Default::default();
    if match_pattern_captured(value, pattern, &mut capture) {
        Some(capture)
    } else {
        None
    }
}

fn match_pattern_captured<'a, 'b>(
    value: &'a Value,
    pattern: &'b Value,
    capture: &mut Capture<'b, 'a>,
) -> bool {
    use Value::*;

    match pattern {
        // Concrete value.
        Null | Bool(_) | Number(_) => value == pattern,

        // "_" matches anything.
        String(s) => s == "_" || value == pattern,

        // Treat array as meaningful expressions.
        Array(v) => {
            if let Some(String(op_name)) = v.get(0) {
                match op_name.as_ref() {
                    "or" => v[1..]
                        .iter()
                        .any(|pat| match_pattern_captured(value, pat, capture)),
                    "and" => v[1..]
                        .iter()
                        .all(|pat| match_pattern_captured(value, pat, capture)),
                    "not" if v.len() == 2 => !match_pattern_captured(value, &v[1], capture),
                    "range" if v.len() == 3 => {
                        if let (Number(start), Number(end), Number(value)) = (&v[1], &v[2], value) {
                            // Unfortunately, Number does not implement PartialOrd.
                            if let (Some(start), Some(end), Some(value)) =
                                (start.as_u64(), end.as_u64(), value.as_u64())
                            {
                                start <= value && value <= end
                            } else if let (Some(start), Some(end), Some(value)) =
                                (start.as_i64(), end.as_i64(), value.as_i64())
                            {
                                start <= value && value <= end
                            } else if let (Some(start), Some(end), Some(value)) =
                                (start.as_f64(), end.as_f64(), value.as_f64())
                            {
                                start <= value && value <= end
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    }
                    "prefix" => {
                        if let Array(value) = value {
                            v[1..]
                                .iter()
                                .enumerate()
                                .all(|(i, pat)| match_pattern_captured(&value[i], pat, capture))
                        } else {
                            false
                        }
                    }
                    "contain" if v.len() == 2 => {
                        if let Array(value) = value {
                            value
                                .iter()
                                .any(|value| match_pattern_captured(value, &v[1], capture))
                        } else {
                            false
                        }
                    }
                    "capture" if v.len() == 3 => {
                        if let String(captured_name) = &v[1] {
                            let pattern = &v[2];
                            if match_pattern_captured(value, pattern, capture) {
                                capture.insert(captured_name, value);
                                true
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    }
                    _ => false,
                }
            } else {
                // Not an expression.
                false
            }
        }

        // Match if pattern is a subset of the target.
        Object(m) => m.iter().all(|(k, pat)| {
            value
                .get(k)
                .map(|value| match_pattern_captured(value, pat, capture))
                .unwrap_or(false)
        }),
    }
}

pub type Capture<'k, 'v> = HashMap<&'k str, &'v Value>;
