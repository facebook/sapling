/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <sqlite3.h>

#include <folly/Synchronized.h>

namespace facebook::eden {

enum class SqliteDbStatus { NOT_YET_OPENED, FAILED_TO_OPEN, OPEN, CLOSED };

struct SqliteConnection {
  sqlite3* db{nullptr};
  SqliteDbStatus status{SqliteDbStatus::NOT_YET_OPENED};
};

using LockedSqliteConnection = folly::Synchronized<SqliteConnection>::LockedPtr;

} // namespace facebook::eden
