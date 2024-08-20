/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use std::borrow::Cow;
use std::fmt;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use command::RetryablePipe;
use mononoke_types::RepositoryId;
use mysql_client::query;
use mysql_client::Connection;
use mysql_client::ToSQL;
use storage::Xdb;

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RepositoryName(String);

impl From<String> for RepositoryName {
    fn from(other: String) -> Self {
        Self(other)
    }
}

impl ToSQL for RepositoryName {
    fn to_sql_string(&self) -> Cow<str> {
        self.0.to_sql_string()
    }
}

impl fmt::Display for RepositoryName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl RepositoryName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RepositoryFingerprint(String);

impl From<String> for RepositoryFingerprint {
    fn from(other: String) -> Self {
        Self(other)
    }
}

impl ToSQL for RepositoryFingerprint {
    fn to_sql_string(&self) -> Cow<str> {
        self.0.to_sql_string()
    }
}

impl RepositoryFingerprint {
    pub fn equals_clause(&self) -> RepositoryFingerprintEqualsClause<'_> {
        RepositoryFingerprintEqualsClause(self)
    }
}

pub struct RepositoryFingerprintEqualsClause<'a>(&'a RepositoryFingerprint);

impl ToSQL for RepositoryFingerprintEqualsClause<'_> {
    fn to_sql_string(&self) -> Cow<str> {
        return query!(
            "= {fingerprint}",
            fingerprint: &RepositoryFingerprint = self.0
        )
        .to_sql_string()
        .into_owned()
        .into();
    }
}

pub struct Repository<'a> {
    id: RepositoryId,
    name: RepositoryName,
    xdb: &'a Xdb,
}

impl<'a> Repository<'a> {
    pub fn new(id: RepositoryId, name: RepositoryName, xdb: &'a Xdb) -> Self {
        Repository { id, name, xdb }
    }

    pub fn id(&self) -> RepositoryId {
        self.id
    }

    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    fn current_mononoke_fingerprint(&self) -> Result<RepositoryFingerprint> {
        let mut pipe = RetryablePipe::new();
        let remote_git_retries = 2;
        let repo_url = format!(
            "ssh://git-ro.vip.facebook.com/data/gitrepos/{}.git",
            self.name()
        );

        pipe.add_retryable_command(
            vec![
                "git",
                "-c",
                "http.extraHeader='x-route-to-mononoke: 1'",
                "ls-remote",
                "--refs",
                &repo_url,
            ],
            remote_git_retries,
        );

        pipe.add_nonretryable_command(vec!["tr", "'\t'", "' '"]);
        pipe.add_nonretryable_command(vec!["sort", "-k", "2"]);
        pipe.add_nonretryable_command(vec!["sha1sum"]);
        pipe.add_nonretryable_command(vec!["awk", "{printf($1)}"]);

        Ok(pipe.spawn_commands()?.into())
    }

    async fn get_metagit_fingerprint(
        &self,
        conn: &mut Connection,
    ) -> Result<Option<RepositoryFingerprint>> {
        let metagit_fingerprint_query = query!(
            r#"
            SELECT fingerprint from repositories
            WHERE
                repositories.name = {name}
            "#,
            name: &RepositoryName = &self.name,
        );

        let (metagit_fingerprint,): (Option<String>,) = conn
            .query(metagit_fingerprint_query)
            .await
            .with_context(|| {
                format!(
                    "Error querying for fingerprint of repository `{}`",
                    self.name()
                )
            })?
            .into_first_row()
            .with_context(|| {
                format!(
                    "Error decoding fingerprint for repository `{}`",
                    self.name()
                )
            })?
            .ok_or_else(|| anyhow!("Repository `{}` does not exist", self.name()))?;

        Ok(metagit_fingerprint.map(|fingerprint| fingerprint.into()))
    }

    pub async fn update_metagit_fingerprint(&self) -> Result<RepositoryFingerprint> {
        let mononoke_git_fingerprint = self.current_mononoke_fingerprint()?;
        logging::debug!(
            "Current Mononoke git fingerprint is `{:?}` for repository `{}`",
            mononoke_git_fingerprint,
            self.name()
        );

        let read_connection_metagit_fingerprint = self
            .get_metagit_fingerprint(&mut self.xdb.read_conn().await?)
            .await?;
        if let Some(resolved_metagit_fingerprint) = read_connection_metagit_fingerprint {
            if resolved_metagit_fingerprint == mononoke_git_fingerprint {
                logging::debug!(
                    "Skipping fingerprint update for repository `{}` since it is same as XDB read replicas",
                    self.name()
                );
                return Ok(mononoke_git_fingerprint);
            }
        }

        let write_connection_metagit_fingerprint = self
            .get_metagit_fingerprint(&mut self.xdb.write_conn().await?)
            .await?;
        if let Some(resolved_metagit_fingerprint) = write_connection_metagit_fingerprint {
            if resolved_metagit_fingerprint == mononoke_git_fingerprint {
                logging::debug!(
                    "Skipping fingerprint update for repository `{}` since it is same as on XDB master instance",
                    self.name()
                );
                return Ok(mononoke_git_fingerprint);
            } else {
                let update_metagit_fingerprint_query = query!(
                r#"
            UPDATE repositories
                SET fingerprint = {fingerprint}, publish_fingerprint_now = NULL
            WHERE
                repositories.name = {name}
                AND repositories.fingerprint {fingerprint_equals}
            "#,
                fingerprint: &RepositoryFingerprint = &mononoke_git_fingerprint,
                name: &RepositoryName = &self.name,
                fingerprint_equals: RepositoryFingerprintEqualsClause = resolved_metagit_fingerprint.equals_clause(),
                );

                let conn = &mut self.xdb.write_conn().await?;

                let res = conn
                    .query(update_metagit_fingerprint_query)
                    .await
                    .with_context(|| {
                        format!(
                            "Error updating fingerprint for repository `{}`",
                            self.name()
                        )
                    })?;

                let num_rows_affected = res.num_rows_affected()?;

                if num_rows_affected == 0 {
                    logging::debug!(
                        "Found existing fingerprint to be different from `{:?}` while trying to update it to `{:?}` for repository `{}`",
                        resolved_metagit_fingerprint,
                        mononoke_git_fingerprint,
                        self.name()
                    );
                } else if num_rows_affected == 1 {
                    logging::debug!(
                        "Updated fingerprint from `{:?}` to `{:?}` for repository `{}`",
                        resolved_metagit_fingerprint,
                        mononoke_git_fingerprint,
                        self.name()
                    );
                } else {
                    return Err(anyhow!(
                        "Unexpected number of rows, precisely `{}`, affected while trying to update fingerprint from `{:?}` to `{:?}` for repository `{}`",
                        num_rows_affected,
                        resolved_metagit_fingerprint,
                        mononoke_git_fingerprint,
                        self.name()
                    ));
                }
            }
        } else {
            let update_metagit_fingerprint_query = query!(
            r#"
            UPDATE repositories
                SET fingerprint = {fingerprint}
            WHERE
                repositories.name = {name}
                AND repositories.fingerprint is NULL
            "#,
            fingerprint: &RepositoryFingerprint = &mononoke_git_fingerprint,
            name: &RepositoryName = &self.name,
            );

            let conn = &mut self.xdb.write_conn().await?;

            let res = conn
                .query(update_metagit_fingerprint_query)
                .await
                .with_context(|| {
                    format!("Error setting fingerprint for repository `{}`", self.name())
                })?;

            let num_rows_affected = res.num_rows_affected()?;

            if num_rows_affected == 0 {
                logging::debug!(
                    "Found existing fingerprint while trying to update it to `{:?}` from NULL for repository `{}`",
                    mononoke_git_fingerprint,
                    self.name()
                );
            } else if num_rows_affected == 1 {
                logging::debug!(
                    "Set fingerprint to `{:?}` for repository `{}`",
                    mononoke_git_fingerprint,
                    self.name()
                );
            } else {
                return Err(anyhow!(
                    "Unexpected number of rows, precisely `{}`, affected while trying to set fingerprint to `{:?}` from NULL for repository `{}`",
                    num_rows_affected,
                    mononoke_git_fingerprint,
                    self.name()
                ));
            }
        }

        Ok(mononoke_git_fingerprint)
    }
}
