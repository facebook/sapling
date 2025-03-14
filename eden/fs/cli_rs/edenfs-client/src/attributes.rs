/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt::Display;
use std::str::FromStr;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use thrift_types::edenfs::FileAttributes;
use thrift_types::fbthrift::ThriftEnum;

pub fn all_attributes() -> &'static [&'static str] {
    FileAttributes::variants()
}

pub fn file_attributes_from_strings<T>(attrs: &[T]) -> Result<i64>
where
    T: AsRef<str> + Display,
{
    attrs
        .iter()
        .map(|attr| {
            FileAttributes::from_str(attr.as_ref())
                .with_context(|| anyhow!("invalid file attribute: {}", attr))
        })
        .try_fold(0, |acc, x| x.map(|y| acc | y.inner_value() as i64))
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_attributes_from_strings() -> Result<()> {
        assert_eq!(file_attributes_from_strings::<String>(&[])?, 0);
        assert_eq!(
            file_attributes_from_strings(&["SHA1_HASH", "SOURCE_CONTROL_TYPE"])?,
            FileAttributes::SHA1_HASH.inner_value() as i64
                | FileAttributes::SOURCE_CONTROL_TYPE.inner_value() as i64
        );
        assert!(file_attributes_from_strings(&["INVALID"]).is_err());
        Ok(())
    }
}
