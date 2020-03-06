/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod constructors;
#[cfg(fbcode_build)]
pub mod facebook;
mod sqlite;

use sql::Transaction;

pub use constructors::{SqlConnections, SqlConstructors};
pub use sqlite::{create_sqlite_connections, open_sqlite_in_memory, open_sqlite_path};

#[must_use]
pub enum TransactionResult {
    Succeeded(Transaction),
    Failed,
}
