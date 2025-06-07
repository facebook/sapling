/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use regex::Regex;

pub fn normalize_hostname(hostname: &str) -> String {
    // Normalizes the hostname of an EdenFS host for telemetry.

    if let Some(stripped) = hostname.strip_suffix(".dhcp.thefacebook.com") {
        return stripped.to_string();
    }

    if hostname.ends_with(".thefacebook.com") {
        return hostname.to_string();
    }

    if hostname.ends_with(".fbinfra.net") {
        return hostname.to_string();
    }

    if let Some(stripped) = hostname.strip_suffix(".facebook.com") {
        return stripped.to_string();
    }

    hostname
        .split_once('.')
        .map_or(hostname, |h| h.0)
        .to_string()
}

pub fn get_host_prefix(hostname: &str) -> String {
    // Get the prefix of a hostname, e.g., "devvm", "od".
    //
    // Returns an empty string if no prefix is found.

    let re = Regex::new(r"([a-zA-Z\-]+)\d+.*").unwrap();
    re.captures(hostname).map_or_else(String::new, |captures| {
        captures.get(1).unwrap().as_str().to_string()
    })
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs::File;
    use std::path::PathBuf;

    use super::get_host_prefix;
    use super::normalize_hostname;

    #[test]
    fn test_normalize_hostname() {
        let test_data = env::var_os("TEST_DATA");
        assert!(test_data.is_some());

        let test_path = PathBuf::from(test_data.unwrap()).join("NormalizedHostnameTestCases.csv");
        let file = File::open(test_path).unwrap();
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(true)
            .from_reader(file);

        for result in reader.records() {
            let record = result.unwrap();
            let hostname = record[0].to_string();
            let normalized_hostname = record[1].to_string();

            assert_eq!(normalize_hostname(&hostname), normalized_hostname);
        }
    }

    #[test]
    fn test_get_host_prefix() {
        let od_prefix = get_host_prefix("od01.abc1");
        assert_eq!(od_prefix, "od");

        let devvm_prefix = get_host_prefix("devvm12345.abc1");
        assert_eq!(devvm_prefix, "devvm");

        let devvm_prefix_long = get_host_prefix("devvm12345.abc1.facebook.com");
        assert_eq!(devvm_prefix_long, "devvm");

        let mbp_prefix = get_host_prefix("helsel-mbp");
        assert_eq!(mbp_prefix, "");
    }
}
