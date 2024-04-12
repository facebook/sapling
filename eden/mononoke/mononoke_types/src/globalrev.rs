/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;
use std::fmt::Display;
use std::str;
use std::str::FromStr;

use abomonation_derive::Abomonation;
use anyhow::bail;
use anyhow::Error;
use anyhow::Result;
use edenapi_types::CommitId as EdenapiCommitId;
use sql::mysql;

use crate::BonsaiChangeset;
use crate::BonsaiChangesetMut;

pub const GLOBALREV_EXTRA: &str = "global_rev";

// Globalrev of first commit when globalrevs were introduced in Mercurial.
// To get globalrev from commit we want to check whether there exists "global_rev" key in bcs extras
// and is not less than START_COMMIT_GLOBALREV.
// Otherwise we try to fetch "convert_revision" key, and parse svnrev from it.
pub const START_COMMIT_GLOBALREV: u64 = 1000147970;

// Changeset globalrev.
#[derive(Abomonation, Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[derive(mysql::OptTryFromRowField)]
pub struct Globalrev(u64);

impl Globalrev {
    pub const fn start_commit() -> Self {
        Self(START_COMMIT_GLOBALREV)
    }

    #[inline]
    pub const fn new(rev: u64) -> Self {
        Self(rev)
    }

    #[inline]
    pub fn id(&self) -> u64 {
        self.0
    }

    // ex. svn:uuid/path@1234
    pub fn parse_svnrev(svnrev: &str) -> Result<u64> {
        let at_pos = svnrev
            .rfind('@')
            .ok_or_else(|| Error::msg("Wrong convert_revision value"))?;
        let result = svnrev[1 + at_pos..].parse::<u64>()?;
        Ok(result)
    }

    pub fn from_bcs(bcs: &BonsaiChangeset) -> Result<Self> {
        match (
            bcs.hg_extra().find(|(key, _)| key == &GLOBALREV_EXTRA),
            bcs.hg_extra().find(|(key, _)| key == &"convert_revision"),
        ) {
            (Some((_, globalrev)), Some((_, svnrev))) => {
                let globalrev = str::from_utf8(globalrev)?.parse::<u64>()?;
                // if we can't parse svnrev, we fallback to globalrev
                let svnrev = Globalrev::parse_svnrev(str::from_utf8(svnrev)?).unwrap_or(globalrev);
                if globalrev >= START_COMMIT_GLOBALREV {
                    Ok(Self::new(globalrev))
                } else {
                    Ok(Self::new(svnrev))
                }
            }
            (Some((_, globalrev)), None) => {
                let globalrev = str::from_utf8(globalrev)?.parse::<u64>()?;
                if globalrev < START_COMMIT_GLOBALREV {
                    bail!("Bonsai cs {:?} without globalrev", bcs)
                } else {
                    Ok(Self::new(globalrev))
                }
            }
            (None, Some((_, svnrev))) => {
                let svnrev = Globalrev::parse_svnrev(str::from_utf8(svnrev)?)?;
                Ok(Self::new(svnrev))
            }
            (None, None) => bail!("Bonsai cs {:?} without globalrev", bcs),
        }
    }

    pub fn set_on_changeset(&self, bcs: &mut BonsaiChangesetMut) {
        bcs.hg_extra.insert(
            GLOBALREV_EXTRA.into(),
            format!("{}", self.id()).into_bytes(),
        );
    }

    #[must_use = "increment does not modify the generation object"]
    pub fn increment(self) -> Self {
        Self(self.0 + 1)
    }
}

impl Display for Globalrev {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        Display::fmt(&self.0, fmt)
    }
}

impl FromStr for Globalrev {
    type Err = <u64 as FromStr>::Err;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        u64::from_str(s).map(Globalrev::new)
    }
}

impl From<Globalrev> for EdenapiCommitId {
    fn from(value: Globalrev) -> Self {
        EdenapiCommitId::Globalrev(value.id())
    }
}

#[cfg(test)]
mod test {
    use anyhow::Error;

    use crate::private::Blake2;
    use crate::BonsaiChangeset;
    use crate::BonsaiChangesetMut;
    use crate::ChangesetId;
    use crate::DateTime;
    use crate::Globalrev;

    fn create_bonsai(
        hg_extra: impl IntoIterator<Item = (String, Vec<u8>)>,
    ) -> Result<BonsaiChangeset, Error> {
        let bcs = BonsaiChangesetMut {
            parents: vec![ChangesetId::new(Blake2::from_byte_array([0x33; 32]))],
            author: "author".into(),
            author_date: DateTime::from_timestamp(0, 0)?,
            committer: None,
            committer_date: None,
            message: "message".into(),
            hg_extra: hg_extra.into_iter().collect(),
            git_extra_headers: None,
            git_tree_hash: None,
            file_changes: Default::default(),
            is_snapshot: false,
            git_annotated_tag: None,
        };
        bcs.freeze()
    }

    #[test]
    fn test_globalrev_from_bcs_should_error_when_not_present() -> Result<(), Error> {
        let bcs = create_bonsai(vec![])?;
        assert!(Globalrev::from_bcs(&bcs).is_err());
        Ok(())
    }

    #[test]
    fn test_globalrev_from_bcs_from_globalrev_extra() -> Result<(), Error> {
        let bcs = create_bonsai(vec![("global_rev".into(), "1012511548".into())])?;
        assert_eq!(Globalrev::new(1012511548), Globalrev::from_bcs(&bcs)?);
        Ok(())
    }

    #[test]
    fn test_globalrev_from_bcs_from_convert_revision() -> Result<(), Error> {
        let bcs = create_bonsai(vec![(
            "convert_revision".into(),
            "svn:uuid/path@9999999993".into(),
        )])?;
        assert_eq!(Globalrev::new(9999999993), Globalrev::from_bcs(&bcs)?);
        Ok(())
    }

    #[test]
    fn test_globalrev_from_bcs_from_both_extras() -> Result<(), Error> {
        let bcs = create_bonsai(vec![
            ("global_rev".into(), "1012511548".into()),
            ("convert_revision".into(), "svn:uuid/path@9999999993".into()),
        ])?;
        assert_eq!(Globalrev::new(1012511548), Globalrev::from_bcs(&bcs)?);
        Ok(())
    }

    #[test]
    fn test_globalrev_from_bcs_from_both_extrasut_and_hg_convert_revision() -> Result<(), Error> {
        let bcs = create_bonsai(vec![
            ("global_rev".into(), "1012511548".into()),
            (
                "convert_revision".into(),
                "3eecc374fc80412b070f0757db694064308aa230".into(),
            ),
        ])?;
        assert_eq!(Globalrev::new(1012511548), Globalrev::from_bcs(&bcs)?);
        Ok(())
    }
}
