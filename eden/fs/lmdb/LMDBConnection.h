/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <lmdb.h> // @manual

#include <folly/Synchronized.h>

namespace facebook::eden {

enum class LMDBDbStatus { NOT_YET_OPENED, FAILED_TO_OPEN, OPEN, CLOSED };

struct LMDBConnection {
  MDB_env* mdb_env_{nullptr};
  MDB_dbi mdb_dbi_;
  MDB_txn* mdb_txn_{nullptr};
  LMDBDbStatus status{LMDBDbStatus::NOT_YET_OPENED};
};

using LockedLMDBConnection = folly::Synchronized<LMDBConnection>::LockedPtr;

} // namespace facebook::eden
