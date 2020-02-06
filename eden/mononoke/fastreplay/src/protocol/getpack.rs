/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use anyhow::Error;
use mercurial_types::HgFileNodeId;
use mononoke_types::MPath;
use std::str::FromStr;

pub struct RequestGetpackArgs {
    pub entries: Vec<(MPath, Vec<HgFileNodeId>)>,
}

impl FromStr for RequestGetpackArgs {
    type Err = Error;

    fn from_str(args: &str) -> Result<Self, Self::Err> {
        let entries = serde_json::from_str::<Vec<(&str, Vec<&str>)>>(&args)?;

        let entries = entries
            .iter()
            .map(|(path, filenodes)| {
                let path = MPath::new(&path)?;
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
