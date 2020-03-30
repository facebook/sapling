/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{format_err, Error};
use bookmarks::BookmarkName;
use clap::ArgMatches;
use cmdlib::args;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::compat::Future01CompatExt;
use mercurial_types::HgChangesetId;
use mononoke_types::{RepositoryId, Timestamp};
use slog::info;
use sql::{queries, Connection};
use sql_ext::SqlConstructors;
use std::borrow::Cow;
use std::collections::HashMap;
use std::convert::TryFrom;
use std::str::FromStr;
use tokio::process::Command;

#[derive(Debug, PartialEq, Eq)]
pub struct HgRecordingEntry {
    pub id: i64,
    pub onto: BookmarkName,
    pub onto_rev: HgChangesetId,
    pub bundle: Vec<u8>,
    pub timestamps: HashMap<HgChangesetId, Timestamp>,
    pub revs: Vec<HgChangesetId>,
}

pub struct HgRecordingClient<'a> {
    bundle_helper: &'a str,
    repo_id: RepositoryId,
    sql: HgRecordingConnection,
}

struct HgRecordingConnection(Connection);

impl SqlConstructors for HgRecordingConnection {
    const LABEL: &'static str = "hg_recording";

    fn from_connections(
        _write_connection: Connection,
        read_connection: Connection,
        _read_master_connection: Connection,
    ) -> Self {
        Self(read_connection)
    }

    fn get_up_query() -> &'static str {
        include_str!("../schemas/pushrebaserecording.sql")
    }
}

queries! {
    read SelectNextSuccessfulHgRecordingEntry(repo_id: RepositoryId, min_id: i64) -> (i64, String, String, Option<String>, String, String) {
        "
        SELECT id, onto, ontorev, bundlehandle, timestamps, ordered_added_revs
        FROM pushrebaserecording
        WHERE repo_id = {repo_id} AND id > {min_id} AND pushrebase_errmsg IS NULL
        ORDER BY id ASC
        LIMIT 1
        "
    }
}

impl<'a> HgRecordingClient<'a> {
    pub async fn new(
        fb: FacebookInit,
        bundle_helper: &'a str,
        matches: &ArgMatches<'_>,
    ) -> Result<HgRecordingClient<'a>, Error> {
        let sql = args::open_sql::<HgRecordingConnection>(fb, matches)
            .compat()
            .await?;
        let repo_id = args::get_repo_id(fb, matches)?;
        Ok(HgRecordingClient {
            bundle_helper,
            repo_id,
            sql,
        })
    }

    pub async fn next_entry(
        &self,
        ctx: &CoreContext,
        min_id: i64,
    ) -> Result<Option<HgRecordingEntry>, Error> {
        let entry =
            SelectNextSuccessfulHgRecordingEntry::query(&self.sql.0, &self.repo_id, &min_id)
                .compat()
                .await?
                .into_iter()
                .next();

        let entry = match entry {
            Some(entry) => entry,
            None => {
                return Ok(None);
            }
        };

        let (id, onto, onto_rev, bundle_handle, timestamps, revs) = entry;
        let onto = BookmarkName::try_from(onto.as_str())?;
        let onto_rev = HgChangesetId::from_str(&onto_rev)?;

        let timestamps = serde_json::from_str::<HashMap<Cow<'_, str>, (f64, u64)>>(&timestamps)?;
        let timestamps = timestamps
            .into_iter()
            .map(|(cs_id, (ts, _))| {
                let cs_id = HgChangesetId::from_str(&cs_id)?;
                let ts = Timestamp::from_timestamp_secs(ts as i64);
                Ok((cs_id, ts))
            })
            .collect::<Result<_, Error>>()?;

        let revs = serde_json::from_str::<Vec<Cow<'_, str>>>(&revs)?;
        let revs = revs
            .into_iter()
            .map(|cs| HgChangesetId::from_str(&cs))
            .collect::<Result<_, Error>>()?;

        let bundle_handle = bundle_handle.ok_or(Error::msg("Bundle handle is missing"))?;

        info!(ctx.logger(), "Fetching bundle: {}", bundle_handle);
        let bundle = self.fetch_bundle(&bundle_handle).await?;

        Ok(Some(HgRecordingEntry {
            id,
            onto,
            onto_rev,
            bundle,
            timestamps,
            revs,
        }))
    }

    async fn fetch_bundle(&self, handle: &str) -> Result<Vec<u8>, Error> {
        // NOTE: We buffer all the output here because we're going to buffer it anyway.
        let output = Command::new(self.bundle_helper)
            .arg(handle)
            .output()
            .await?;

        if !output.status.success() {
            let e = format_err!(
                "Failed to fetch bundle: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            return Err(e);
        }

        Ok(output.stdout)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use maplit::hashmap;
    use mercurial_types_mocks::nodehash::{ONES_CSID, TWOS_CSID};
    use mononoke_types_mocks::repo::REPO_ZERO;
    use serde_json::json;

    queries! {
        write InsertHgRecordingEntry(
            id: i64,
            repo_id: i32,
            onto: String,
            ontorev: String,
            bundlehandle: String,
            timestamps: String,
            ordered_added_revs: String,
            pushrebase_errmsg: Option<String>
        ) {
            none,
            "
            INSERT INTO
            pushrebaserecording(id, repo_id, onto, ontorev, bundlehandle, timestamps, ordered_added_revs, pushrebase_errmsg)
            VALUES ({id}, {repo_id}, {onto}, {ontorev}, {bundlehandle}, {timestamps}, {ordered_added_revs}, {pushrebase_errmsg})
            "
        }
    }

    impl HgRecordingClient<'static> {
        fn test_instance() -> Result<Self, Error> {
            Ok(HgRecordingClient {
                bundle_helper: "printf",
                repo_id: REPO_ZERO,
                sql: HgRecordingConnection::with_sqlite_in_memory()?,
            })
        }
    }

    #[fbinit::compat_test]
    async fn test_next_entry(fb: FacebookInit) -> Result<(), Error> {
        let client = HgRecordingClient::test_instance()?;
        let ctx = CoreContext::test_mock(fb);

        InsertHgRecordingEntry::query(
            &client.sql.0,
            &1,
            &REPO_ZERO.id(),
            &"book123".to_string(),
            &ONES_CSID.to_string(),
            &"handle123".to_string(),
            &json!({ ONES_CSID.to_string(): [123.0, 0] }).to_string(),
            &json!([TWOS_CSID.to_string()]).to_string(),
            &None,
        )
        .compat()
        .await?;

        let entry = client
            .next_entry(&ctx, 0)
            .await?
            .ok_or(Error::msg("Entry not found"))?;

        assert_eq!(entry.id, 1);
        assert_eq!(entry.onto, BookmarkName::try_from("book123")?);
        assert_eq!(entry.onto_rev, ONES_CSID);
        assert_eq!(entry.bundle.as_slice(), "handle123".as_bytes());
        assert_eq!(
            entry.timestamps,
            hashmap! { ONES_CSID => Timestamp::from_timestamp_secs(123) }
        );
        assert_eq!(entry.revs, vec![TWOS_CSID]);

        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_no_entry(fb: FacebookInit) -> Result<(), Error> {
        let client = HgRecordingClient::test_instance()?;
        let ctx = CoreContext::test_mock(fb);

        assert_eq!(client.next_entry(&ctx, 0).await?, None);
        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_excluded_entry(fb: FacebookInit) -> Result<(), Error> {
        let client = HgRecordingClient::test_instance()?;
        let ctx = CoreContext::test_mock(fb);

        InsertHgRecordingEntry::query(
            &client.sql.0,
            &1,
            &REPO_ZERO.id(),
            &"book123".to_string(),
            &ONES_CSID.to_string(),
            &"handle123".to_string(),
            &"{}".to_string(),
            &"[]".to_string(),
            &None,
        )
        .compat()
        .await?;

        assert_eq!(client.next_entry(&ctx, 1).await?, None);
        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_error_entry(fb: FacebookInit) -> Result<(), Error> {
        let client = HgRecordingClient::test_instance()?;
        let ctx = CoreContext::test_mock(fb);

        assert_eq!(client.next_entry(&ctx, 0).await?, None);
        Ok(())
    }
}
