/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use mercurial_types::HgFileNodeId;
use mononoke_types::MPath;
use std::borrow::Cow;
use std::str::FromStr;

pub struct RequestGetpackArgs {
    pub entries: Vec<(MPath, Vec<HgFileNodeId>)>,
}

type ReplayData<'a> = Vec<(Cow<'a, str>, Vec<Cow<'a, str>>)>;

impl FromStr for RequestGetpackArgs {
    type Err = Error;

    fn from_str<'a>(args: &'a str) -> Result<Self, Self::Err> {
        let entries = serde_json::from_str::<ReplayData<'a>>(&args)?;

        let entries = entries
            .iter()
            .map(|(path, filenodes)| {
                let path = MPath::new(path.as_ref())?;
                let filenodes = filenodes
                    .iter()
                    .map(|n| HgFileNodeId::from_str(&n))
                    .collect::<Result<Vec<_>, Error>>()?;

                Ok((path, filenodes))
            })
            .collect::<Result<Vec<_>, Error>>()?;

        Ok(RequestGetpackArgs { entries })
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_parse() -> Result<(), Error> {
        RequestGetpackArgs::from_str(include_str!("./fixtures/getpack.json"))?;
        Ok(())
    }
}
