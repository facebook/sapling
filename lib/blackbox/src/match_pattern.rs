// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Pattern matching for JSON value.

use serde_json::Value;

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
                    "or" => v[1..].iter().any(|pat| match_pattern(value, pat)),
                    "and" => v[1..].iter().all(|pat| match_pattern(value, pat)),
                    "not" if v.len() == 2 => !match_pattern(value, &v[1]),
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
                                .all(|(i, pat)| match_pattern(&value[i], pat))
                        } else {
                            false
                        }
                    }
                    "contain" if v.len() == 2 => {
                        if let Array(value) = value {
                            value.iter().any(|value| match_pattern(value, &v[1]))
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
                .map(|value| match_pattern(value, pat))
                .unwrap_or(false)
        }),
    }
}
