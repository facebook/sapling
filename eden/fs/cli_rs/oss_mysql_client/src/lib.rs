/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! OSS-compatible mysql_client replacement for edenfs-saved-state
//!
//! ************************************************************************
//! * WARNING: This crate is (likely) non-functional and is a development vessel for
//!   OSS implementation of a MySQL client. It is not intended for production use.
//! 
//! Steps:
//! 1. Implement interface compatible with internal mysql_client crate
//! 2. Update edenfs-saved-state to use this crate instead of internal mysql_client
//! <WE ARE HERE>
//! 3. Build and test in OSS environment
//! 4. Create contract tests that run on both internal and OSS mysql_client crates
//! 
//! * END WARNING
//! ************************************************************************
//! 
//! This crate provides a minimal MySQL client API compatible with the internal
//! mysql_client crate, but implemented using OSS-compatible libraries.

use std::fmt;

use anyhow::Context;
use anyhow::Result;
use fbinit::FacebookInit;
use mysql_async::prelude::*;
use mysql_async::{Pool, Row};

/// Database locator that specifies which database to connect to
#[derive(Debug, Clone)]
pub struct DbLocator {
    pub schema: String,
    pub instance_requirement: InstanceRequirement,
}

impl DbLocator {
    pub fn new(schema: &str, instance_requirement: InstanceRequirement) -> Result<Self> {
        Ok(Self {
            schema: schema.to_string(),
            instance_requirement,
        })
    }
}

/// Instance requirement for database connections
#[derive(Debug, Clone, Copy)]
pub enum InstanceRequirement {
    Master,
    Replica,
}

/// A MySQL query with parameter bindings
#[derive(Debug, Clone)]
pub struct Query {
    sql: String,
    params: Vec<mysql_async::Value>,
}

impl Query {
    pub fn new(sql: &str) -> Self {
        Self {
            sql: sql.to_string(),
            params: Vec::new(),
        }
    }

    pub fn add(mut self, other: Query) -> Self {
        // Append the SQL and combine parameters
        self.sql.push(' ');
        self.sql.push_str(&other.sql);
        self.params.extend(other.params);
        self
    }

    pub fn bind<T: Into<mysql_async::Value>>(mut self, param: T) -> Self {
        self.params.push(param.into());
        self
    }
}

/// Query result that can be converted to various types
pub struct QueryResult {
    rows: Vec<Row>,
}

impl QueryResult {
    pub fn into_rows<T: FromRow>(self) -> Result<Vec<T>> {
        let results: Vec<T> = self.rows
            .into_iter()
            .map(T::from_row)
            .collect();
        Ok(results)
    }
}

/// Main MySQL client
pub struct MysqlCppClient {
    pool: Pool,
}

impl MysqlCppClient {
    pub fn new(_fb: FacebookInit) -> Result<Self> {
        // For OSS usage, we'll use a simple localhost MySQL connection
        // In a real deployment, this would be configured via environment or config files
        let database_url = std::env::var("MYSQL_DATABASE_URL")
            .unwrap_or_else(|_| "mysql://root@localhost/test".to_string());

        let opts = mysql_async::Opts::from_url(&database_url)
            .context("Failed to parse MySQL URL")?;

        let pool = Pool::new(opts);

        Ok(Self { pool })
    }

    pub async fn query(&self, locator: &DbLocator, query: Query) -> Result<QueryResult> {
        let mut conn = self.pool.get_conn().await
            .context("Failed to get MySQL connection")?;

        // Switch to the specified schema
        let use_schema = format!("USE `{}`", locator.schema);
        conn.query_drop(use_schema).await
            .context("Failed to switch to schema")?;

        // Execute the query with parameters
        let rows: Vec<Row> = conn.exec(&query.sql, query.params).await
            .context("Failed to execute query")?;

        Ok(QueryResult { rows })
    }
}

/// Macro to create parameterized queries
#[macro_export]
macro_rules! query {
    ($sql:expr) => {
        $crate::Query::new($sql)
    };
    ($sql:expr, $($param_name:ident = $param_value:expr),* $(,)?) => {{
        let mut sql_string = $sql.to_string();
        $(
            // Replace parameter placeholders with ? for mysql_async compatibility
            let param_placeholder = format!("{{{}}}", stringify!($param_name));
            sql_string = sql_string.replace(&param_placeholder, "?");
        )*
        let mut query = $crate::Query::new(&sql_string);
        $(
            query = query.bind($param_value);
        )*
        query
    }};
}

/// Error types for MySQL operations
#[derive(Debug)]
pub struct MysqlError {
    pub message: String,
}

impl fmt::Display for MysqlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "MySQL error: {}", self.message)
    }
}

impl std::error::Error for MysqlError {}

impl From<mysql_async::Error> for MysqlError {
    fn from(err: mysql_async::Error) -> Self {
        Self {
            message: err.to_string(),
        }
    }
}
