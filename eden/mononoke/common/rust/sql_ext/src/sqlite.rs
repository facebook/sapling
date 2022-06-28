/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use sql::rusqlite::Connection as SqliteConnection;
use sql::rusqlite::OpenFlags as SqliteOpenFlags;
use std::fs::create_dir_all;
use std::path::Path;
use std::time::Duration;

fn sqlite_setup_connection(con: &SqliteConnection) {
    // By default, when there's a read/write contention, SQLite will not wait,
    // but rather throw a `SQLITE_BUSY` error. See https://www.sqlite.org/lockingv3.html
    // This means that tests will fail in cases when production setup (e.g. one with MySQL)
    // would not. To change that, let's make sqlite wait for some time, before erroring out
    let _ = con.busy_timeout(Duration::from_secs(10));

    // By default, the `LIKE` operator is case-insensitive.  This doesn't
    // match MySQL, so change it to case-sensitive.
    let _ = con.pragma_update(None, "case_sensitive_like", &true);
}

// Open a single sqlite connection to a new in memory database
pub fn open_sqlite_in_memory() -> Result<SqliteConnection> {
    let con = SqliteConnection::open_in_memory()?;
    sqlite_setup_connection(&con);
    Ok(con)
}

/// Open a single sqlite connection. The Sqlite DB will be created at path if necessary.
pub fn open_sqlite_path<P: AsRef<Path>>(path: P, readonly: bool) -> Result<SqliteConnection> {
    let path = path.as_ref();
    let con = {
        // Open a RW connection with create-on-open enabled, so that the Sqlite DB is initialized
        // even if we then re-open it with RO access. (You cannot open RO with create-on-open)
        // This is useful for tests that want to verify that nothing is written to the DB even when
        // it is empty.
        if let Some(parent) = path.parent() {
            create_dir_all(parent)?;
        }
        let flags = SqliteOpenFlags::SQLITE_OPEN_READ_WRITE | SqliteOpenFlags::SQLITE_OPEN_CREATE;

        SqliteConnection::open_with_flags(&path, flags)?
    };

    let con = if readonly {
        let flags = SqliteOpenFlags::SQLITE_OPEN_READ_ONLY;
        SqliteConnection::open_with_flags(path, flags)?
    } else {
        con
    };

    sqlite_setup_connection(&con);
    Ok(con)
}

/// Open a single sqlite connection. The Sqlite DB must already exist at path.
pub fn open_existing_sqlite_path<P: AsRef<Path>>(
    path: P,
    readonly: bool,
) -> Result<SqliteConnection> {
    let path = path.as_ref();
    let flags = if readonly {
        SqliteOpenFlags::SQLITE_OPEN_READ_ONLY
    } else {
        // NB no creation flag, as db should already exist at this point
        SqliteOpenFlags::SQLITE_OPEN_READ_WRITE
    };
    let con = SqliteConnection::open_with_flags(path, flags)?;
    sqlite_setup_connection(&con);
    Ok(con)
}
