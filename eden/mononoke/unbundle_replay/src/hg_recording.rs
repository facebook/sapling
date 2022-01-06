/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use bookmarks::BookmarkName;
use cached_config::ConfigStore;
use cmdlib::args::{self, MononokeMatches};
use context::CoreContext;
use fbinit::FacebookInit;
use mercurial_types::HgChangesetId;
use mononoke_types::{RepositoryId, Timestamp};
use sql::{queries, Connection};
use sql_construct::{SqlConstruct, SqlConstructFromMetadataDatabaseConfig};
use sql_ext::SqlConnections;
use std::borrow::Cow;
use std::collections::HashMap;
use std::str::FromStr;
use std::time::Duration;

pub struct HgRecordingEntry {
    pub id: i64,
    pub onto: BookmarkName,
    pub onto_rev: HgChangesetId,
    pub bundle: String,
    pub timestamps: HashMap<HgChangesetId, Timestamp>,
    pub revs: Vec<HgChangesetId>,
    pub duration: Option<Duration>,
}

pub struct HgRecordingClient {
    repo_id: RepositoryId,
    sql: HgRecordingConnection,
}

struct HgRecordingConnection(Connection);

impl SqlConstruct for HgRecordingConnection {
    const LABEL: &'static str = "hg_recording";

    const CREATION_QUERY: &'static str = include_str!("../schemas/pushrebaserecording.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self(connections.read_connection)
    }
}

impl SqlConstructFromMetadataDatabaseConfig for HgRecordingConnection {}

type EntryRow = (
    i64,
    String,
    String,
    Option<String>,
    String,
    String,
    Option<i64>,
);

queries! {
    read SelectNextSuccessfulHgRecordingEntryById(repo_id: RepositoryId, min_id: i64) -> (i64, String, String, Option<String>, String, String, Option<i64>) {
        "
        SELECT id, onto, ontorev, bundlehandle, timestamps, ordered_added_revs, duration_ms
        FROM pushrebaserecording
        WHERE repo_id = {repo_id} AND id > {min_id} AND pushrebase_errmsg IS NULL AND conflicts IS NULL
        ORDER BY id ASC
        LIMIT 1
        "
    }

    read SelectNextSuccessfulHgRecordingEntryByOnto(repo_id: RepositoryId, onto: BookmarkName, ontorev: HgChangesetId) -> (i64, String, String, Option<String>, String, String, Option<i64>) {
        "
        SELECT id, onto, ontorev, bundlehandle, timestamps, ordered_added_revs, duration_ms
        FROM pushrebaserecording
        WHERE repo_id = {repo_id} AND onto LIKE {onto} AND ontorev LIKE LOWER(HEX({ontorev})) AND pushrebase_errmsg IS NULL AND conflicts IS NULL
        ORDER BY id ASC
        LIMIT 1
        "
    }
}

impl HgRecordingClient {
    pub fn new<'a>(
        fb: FacebookInit,
        config_store: &ConfigStore,
        matches: &'a MononokeMatches<'a>,
    ) -> Result<HgRecordingClient, Error> {
        let sql = args::open_sql::<HgRecordingConnection>(fb, config_store, matches)?;
        let repo_id = args::get_repo_id(config_store, matches)?;
        Ok(HgRecordingClient { repo_id, sql })
    }

    pub async fn next_entry_by_id(
        &self,
        _ctx: &CoreContext,
        min_id: i64,
    ) -> Result<Option<HgRecordingEntry>, Error> {
        let entry =
            SelectNextSuccessfulHgRecordingEntryById::query(&self.sql.0, &self.repo_id, &min_id)
                .await?
                .into_iter()
                .next();

        self.hydrate_entry(entry).await
    }

    pub async fn next_entry_by_onto(
        &self,
        _ctx: &CoreContext,
        onto: &BookmarkName,
        onto_rev: &HgChangesetId,
    ) -> Result<Option<HgRecordingEntry>, Error> {
        let entry = SelectNextSuccessfulHgRecordingEntryByOnto::query(
            &self.sql.0,
            &self.repo_id,
            onto,
            onto_rev,
        )
        .await?
        .into_iter()
        .next();

        self.hydrate_entry(entry).await
    }

    async fn hydrate_entry(
        &self,
        entry: Option<EntryRow>,
    ) -> Result<Option<HgRecordingEntry>, Error> {
        let entry = match entry {
            Some(entry) => entry,
            None => {
                return Ok(None);
            }
        };

        let (id, onto, onto_rev, bundle, timestamps, revs, duration_ms) = entry;
        let onto = BookmarkName::try_from(onto.as_str())?;
        let onto_rev = HgChangesetId::from_str(&onto_rev)?;
        let duration: Option<Duration> = duration_ms
            .map(|d| Result::<_, Error>::Ok(Duration::from_millis(d.try_into()?)))
            .transpose()?;

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

        let bundle = bundle.ok_or(Error::msg("Bundle handle is missing"))?;

        Ok(Some(HgRecordingEntry {
            id,
            onto,
            onto_rev,
            bundle,
            timestamps,
            revs,
            duration,
        }))
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
        ) {
            none,
            "
            INSERT INTO
            pushrebaserecording(id, repo_id, onto, ontorev, bundlehandle, timestamps, ordered_added_revs)
            VALUES ({id}, {repo_id}, {onto}, {ontorev}, {bundlehandle}, {timestamps}, {ordered_added_revs})
            "
        }

        write SetPushrebaseErrMsgOnAllEntries() {
            none,
            "UPDATE pushrebaserecording SET pushrebase_errmsg = 'oops'"
        }

        write SetConflictsOnAllEntries() {
            none,
            "UPDATE pushrebaserecording SET conflicts = 'oops'"
        }
    }

    impl HgRecordingClient {
        fn test_instance() -> Result<Self, Error> {
            Ok(HgRecordingClient {
                repo_id: REPO_ZERO,
                sql: HgRecordingConnection::with_sqlite_in_memory()?,
            })
        }
    }

    async fn insert_test_entry(client: &HgRecordingClient) -> Result<(), Error> {
        InsertHgRecordingEntry::query(
            &client.sql.0,
            &1,
            &REPO_ZERO.id(),
            &"book123".to_string(),
            &ONES_CSID.to_string(),
            &"handle123".to_string(),
            &json!({ ONES_CSID.to_string(): [123.0, 0] }).to_string(),
            &json!([TWOS_CSID.to_string()]).to_string(),
        )
        .await?;

        Ok(())
    }

    fn assert_is_test_entry(entry: HgRecordingEntry) -> Result<(), Error> {
        assert_eq!(entry.id, 1);
        assert_eq!(entry.onto, BookmarkName::try_from("book123")?);
        assert_eq!(entry.onto_rev, ONES_CSID);
        assert_eq!(&entry.bundle, "handle123");
        assert_eq!(
            entry.timestamps,
            hashmap! { ONES_CSID => Timestamp::from_timestamp_secs(123) }
        );
        assert_eq!(entry.revs, vec![TWOS_CSID]);

        Ok(())
    }

    #[fbinit::test]
    async fn test_next_entry_by_id(fb: FacebookInit) -> Result<(), Error> {
        let client = HgRecordingClient::test_instance()?;
        let ctx = CoreContext::test_mock(fb);

        insert_test_entry(&client).await?;

        let entry = client
            .next_entry_by_id(&ctx, 0)
            .await?
            .ok_or(Error::msg("Entry not found"))?;

        assert_is_test_entry(entry)?;

        Ok(())
    }

    #[fbinit::test]
    async fn test_no_entry_by_id(fb: FacebookInit) -> Result<(), Error> {
        let client = HgRecordingClient::test_instance()?;
        let ctx = CoreContext::test_mock(fb);

        assert!(client.next_entry_by_id(&ctx, 0).await?.is_none());
        Ok(())
    }

    #[fbinit::test]
    async fn test_excluded_entry_by_id(fb: FacebookInit) -> Result<(), Error> {
        let client = HgRecordingClient::test_instance()?;
        let ctx = CoreContext::test_mock(fb);

        insert_test_entry(&client).await?;

        assert!(client.next_entry_by_id(&ctx, 1).await?.is_none());
        Ok(())
    }

    #[fbinit::test]
    async fn test_error_entry_by_id(fb: FacebookInit) -> Result<(), Error> {
        let client = HgRecordingClient::test_instance()?;
        let ctx = CoreContext::test_mock(fb);

        insert_test_entry(&client).await?;
        SetPushrebaseErrMsgOnAllEntries::query(&client.sql.0).await?;

        assert!(client.next_entry_by_id(&ctx, 0).await?.is_none());
        Ok(())
    }

    #[fbinit::test]
    async fn test_conflict_entry_by_id(fb: FacebookInit) -> Result<(), Error> {
        let client = HgRecordingClient::test_instance()?;
        let ctx = CoreContext::test_mock(fb);

        insert_test_entry(&client).await?;
        SetConflictsOnAllEntries::query(&client.sql.0).await?;

        assert!(client.next_entry_by_id(&ctx, 0).await?.is_none());
        Ok(())
    }

    #[fbinit::test]
    async fn test_select_onto(fb: FacebookInit) -> Result<(), Error> {
        let client = HgRecordingClient::test_instance()?;
        let ctx = CoreContext::test_mock(fb);

        insert_test_entry(&client).await?;

        let book = BookmarkName::try_from("book123")?;

        let entry = client
            .next_entry_by_onto(&ctx, &book, &ONES_CSID)
            .await?
            .ok_or(Error::msg("Entry not found"))?;

        assert_is_test_entry(entry)?;

        Ok(())
    }

    #[fbinit::test]
    async fn test_error_entry_onto(fb: FacebookInit) -> Result<(), Error> {
        let client = HgRecordingClient::test_instance()?;
        let ctx = CoreContext::test_mock(fb);

        insert_test_entry(&client).await?;
        SetPushrebaseErrMsgOnAllEntries::query(&client.sql.0).await?;

        let book = BookmarkName::try_from("book123")?;

        assert!(
            client
                .next_entry_by_onto(&ctx, &book, &ONES_CSID)
                .await?
                .is_none()
        );

        Ok(())
    }

    #[fbinit::test]
    async fn test_conflict_entry_onto(fb: FacebookInit) -> Result<(), Error> {
        let client = HgRecordingClient::test_instance()?;
        let ctx = CoreContext::test_mock(fb);

        insert_test_entry(&client).await?;
        SetConflictsOnAllEntries::query(&client.sql.0).await?;

        let book = BookmarkName::try_from("book123")?;

        assert!(
            client
                .next_entry_by_onto(&ctx, &book, &ONES_CSID)
                .await?
                .is_none()
        );

        Ok(())
    }
}
