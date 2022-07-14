/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::CommitsInBundle;
use crate::Repo;
use anyhow::format_err;
use anyhow::Error;
use bonsai_globalrev_mapping::BonsaiGlobalrevMappingEntry;
use bonsai_globalrev_mapping::BonsaiGlobalrevMappingRef;
use cloned::cloned;
use context::CoreContext;
use futures::stream;
use futures::StreamExt;
use futures::TryStreamExt;
use sql::Connection;
use sql_construct::SqlConstruct;
use sql_ext::SqlConnections;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone)]
pub enum GlobalrevSyncer {
    Noop,
    Darkstorm(Arc<DarkstormGlobalrevSyncer>),
}

pub struct DarkstormGlobalrevSyncer {
    orig_repo: Repo,
    darkstorm_repo: Repo,
}

#[derive(Clone)]
struct HgsqlConnection {
    #[allow(dead_code)]
    connection: Connection,
}

impl SqlConstruct for HgsqlConnection {
    const LABEL: &'static str = "globalrev-syncer";

    const CREATION_QUERY: &'static str = include_str!("../schemas/hgsql.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self {
            connection: connections.write_connection,
        }
    }
}

impl GlobalrevSyncer {
    pub fn darkstorm(orig_repo: &Repo, darkstorm_repo: &Repo) -> Self {
        Self::Darkstorm(Arc::new(DarkstormGlobalrevSyncer {
            orig_repo: orig_repo.clone(),
            darkstorm_repo: darkstorm_repo.clone(),
        }))
    }

    pub async fn sync(&self, ctx: &CoreContext, commits: &CommitsInBundle) -> Result<(), Error> {
        match self {
            Self::Noop => Ok(()),
            Self::Darkstorm(syncer) => syncer.sync(ctx, commits).await,
        }
    }
}

impl DarkstormGlobalrevSyncer {
    pub async fn sync(&self, ctx: &CoreContext, commits: &CommitsInBundle) -> Result<(), Error> {
        let commits = match commits {
            CommitsInBundle::Commits(commits) => commits,
            CommitsInBundle::Unknown => {
                return Err(format_err!(
                    "can't use darkstorm globalrev syncer because commits that were \
                    sent in the bundle are not known"
                ));
            }
        };

        let commits = commits.clone().into_iter().map(|(_, bcs_id)| bcs_id);
        let bcs_id_to_globalrev = stream::iter(commits.map(|bcs_id| {
            cloned!(ctx, self.orig_repo);
            async move {
                let maybe_globalrev = orig_repo
                    .bonsai_globalrev_mapping()
                    .get_globalrev_from_bonsai(&ctx, bcs_id)
                    .await?;
                Result::<_, Error>::Ok((bcs_id, maybe_globalrev))
            }
        }))
        .map(Ok)
        .try_buffer_unordered(100)
        .try_filter_map(|(bcs_id, maybe_globalrev)| {
            let bcs_id = bcs_id.clone();
            let maybe_globalrev = maybe_globalrev.clone();
            async move { Ok(maybe_globalrev.map(|globalrev| (bcs_id, globalrev))) }
        })
        .try_collect::<HashMap<_, _>>()
        .await?;

        let entries = bcs_id_to_globalrev
            .into_iter()
            .map(|(bcs_id, globalrev)| BonsaiGlobalrevMappingEntry { bcs_id, globalrev })
            .collect::<Vec<_>>();

        self.darkstorm_repo
            .bonsai_globalrev_mapping()
            .bulk_import(ctx, &entries[..])
            .await?;

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use bonsai_globalrev_mapping::BonsaiGlobalrevMappingEntry;
    use fbinit::FacebookInit;
    use mercurial_types_mocks::globalrev::GLOBALREV_ONE;
    use mercurial_types_mocks::globalrev::GLOBALREV_TWO;
    use mercurial_types_mocks::nodehash::ONES_CSID as ONES_HG_CSID;
    use mercurial_types_mocks::nodehash::TWOS_CSID as TWOS_HG_CSID;
    use mononoke_types::RepositoryId;
    use mononoke_types_mocks::changesetid::ONES_CSID;
    use mononoke_types_mocks::changesetid::TWOS_CSID;
    use test_repo_factory::TestRepoFactory;

    #[fbinit::test]
    async fn test_sync_darkstorm(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);

        let orig_repo: Repo = TestRepoFactory::new(fb)?
            .with_id(RepositoryId::new(0))
            .build()?;
        let darkstorm_repo: Repo = TestRepoFactory::new(fb)?
            .with_id(RepositoryId::new(1))
            .build()?;

        let e1 = BonsaiGlobalrevMappingEntry {
            bcs_id: ONES_CSID,
            globalrev: GLOBALREV_ONE,
        };

        let e2 = BonsaiGlobalrevMappingEntry {
            bcs_id: TWOS_CSID,
            globalrev: GLOBALREV_TWO,
        };
        orig_repo
            .bonsai_globalrev_mapping()
            .bulk_import(&ctx, &[e1, e2])
            .await?;

        let syncer = DarkstormGlobalrevSyncer {
            orig_repo,
            darkstorm_repo: darkstorm_repo.clone(),
        };

        assert!(
            darkstorm_repo
                .bonsai_globalrev_mapping()
                .get_globalrev_from_bonsai(&ctx, ONES_CSID)
                .await?
                .is_none()
        );
        assert!(
            darkstorm_repo
                .bonsai_globalrev_mapping()
                .get_globalrev_from_bonsai(&ctx, TWOS_CSID)
                .await?
                .is_none()
        );
        syncer
            .sync(
                &ctx,
                &CommitsInBundle::Commits(vec![
                    (ONES_HG_CSID, ONES_CSID),
                    (TWOS_HG_CSID, TWOS_CSID),
                ]),
            )
            .await?;

        assert_eq!(
            Some(GLOBALREV_ONE),
            darkstorm_repo
                .bonsai_globalrev_mapping()
                .get_globalrev_from_bonsai(&ctx, ONES_CSID)
                .await?
        );
        assert_eq!(
            Some(GLOBALREV_TWO),
            darkstorm_repo
                .bonsai_globalrev_mapping()
                .get_globalrev_from_bonsai(&ctx, TWOS_CSID)
                .await?
        );
        Ok(())
    }
}
