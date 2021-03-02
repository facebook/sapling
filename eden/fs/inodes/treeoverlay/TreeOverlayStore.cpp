/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/treeoverlay/TreeOverlayStore.h"

#include <folly/FixedString.h>
#include <folly/Range.h>
#include "eden/fs/inodes/InodeNumber.h"
#include "eden/fs/inodes/overlay/gen-cpp2/overlay_types.h"
#include "eden/fs/utils/DirType.h"

namespace facebook::eden {

namespace {

// SQLite table names
constexpr folly::StringPiece kEntryTable = "entries";
constexpr folly::StringPiece kMetadataTable = "metadata";

// Filename of the tree overlay database
constexpr PathComponentPiece kTreeStorePath =
    PathComponentPiece{"treestore.db"};

// Initial Inode ID is root ID + 1
constexpr auto kInitialNodeId = kRootNodeId.getRawValue() + 1;

// Schema version of the SQLite database, everytime we changes the schema we
// must bump this number.
constexpr uint32_t kSchemaVersion = 1;

// Maximum number of values when we do batch insertion
constexpr size_t kBatchInsertSize = 8;

} // namespace

TreeOverlayStore::TreeOverlayStore(AbsolutePathPiece path) {
  ensureDirectoryExists(path);

  db_ = std::make_unique<SqliteDatabase>(path + kTreeStorePath);
}

TreeOverlayStore::TreeOverlayStore(std::unique_ptr<SqliteDatabase> db)
    : db_{std::move(db)} {}

TreeOverlayStore::~TreeOverlayStore() {
  close();
}

void TreeOverlayStore::close() {
  if (db_) {
    db_->close();
  }
}

void TreeOverlayStore::createTableIfNonExisting() {
  // TODO: check `user_version` and migrate schema if necessary
  db_->transaction([&](auto& txn) {
    // `name` column in this table being `STRING` data type essentially capped
    // our ability to support non-UTF-8 path. Currently we do enforce this rule
    // elsewhere but moving forward if we ever need to support non-UTF-8 path we
    // would need to migrate this column.
    SqliteStatement(
        txn,
        "CREATE TABLE IF NOT EXISTS ",
        kEntryTable,
        R"(
  (
    parent INTEGER NOT NULL,
    name STRING NOT NULL,
    dtype INTEGER NOT NULL,
    inode INTEGER NOT NULL,
    sequence_id INTEGER NOT NULL,
    hash BLOB,
    PRIMARY KEY (parent, name)
) WITHOUT ROWID;
  )")
        .step();

    // This is an optimization for future. If we want to implement readdir
    // support in overlay, we would be adding queries to filter by sequence_id.
    SqliteStatement(
        txn,
        "CREATE INDEX IF NOT EXISTS entries_sequence_id_idx ON ",
        kEntryTable,
        " (sequence_id)")
        .step();

    // Optimizing `max(inode)`
    SqliteStatement(
        txn,
        "CREATE INDEX IF NOT EXISTS entries_inode_idx ON ",
        kEntryTable,
        " (inode)")
        .step();

    // Metadata table
    SqliteStatement(txn, "CREATE TABLE IF NOT EXISTS ", kMetadataTable, R"(
  (
     inode INTEGER UNIQUE PRIMARY KEY NOT NULL,
    mode INTEGER NOT NULL,
    uid INTEGER NOT NULL,
    gid INTEGER NOT NULL,
    atime INTEGER NOT NULL,
    mtime INTEGER NOT NULL,
    ctime INTEGER NOT NULL
) WITHOUT ROWID;
  )")
        .step();

    SqliteStatement(txn, "PRAGMA user_version = ", kSchemaVersion).step();
  });
}

InodeNumber TreeOverlayStore::loadCounters() {
  // load ids
  auto db = db_->lock();

  {
    auto stmt =
        SqliteStatement(db, "SELECT max(sequence_id) FROM ", kEntryTable);

    if (stmt.step()) {
      nextEntryId_ = stmt.columnUint64(0) + 1;
    } else {
      throw std::runtime_error("unable to get max(sequence_id) from the table");
    }
  }

  {
    auto stmt = SqliteStatement(db, "SELECT max(inode) FROM ", kEntryTable);

    if (stmt.step()) {
      auto inode = stmt.columnUint64(0);
      if (inode == 0) {
        nextInode_ = kInitialNodeId;
      } else {
        nextInode_ = inode + 1;
      }
    } else {
      throw std::runtime_error("unable to get max(inode) from the table");
    }
  }

  return InodeNumber{nextInode_.load()};
}

InodeNumber TreeOverlayStore::nextInodeNumber() {
  return InodeNumber{nextInode_.fetch_add(1, std::memory_order_acq_rel)};
}

namespace {
SqliteStatement makeBatchInsert(SqliteDatabase::Connection& db, size_t size) {
  auto insert_template = folly::to<std::string>(
      "INSERT INTO ",
      kEntryTable,
      " (parent, name, dtype, inode, sequence_id, hash) VALUES ");
  auto values_fmt = folly::makeFixedString("(?,?,?,?,?,?)");

  fmt::memory_buffer stmt_buffer;
  // The length of final statement is the sum of:
  // - "INSERT ... VALUES"
  // - "(?,...,?)" * number of entries
  // - "," * (number of entries - 1)
  stmt_buffer.reserve(
      insert_template.length() + size * values_fmt.length() + (size - 1));
  fmt::format_to(stmt_buffer, insert_template.c_str());

  for (size_t i = 0; i < size; i++) {
    if (i != 0) {
      fmt::format_to(stmt_buffer, ","); // delimiter
    }
    fmt::format_to(stmt_buffer, values_fmt.c_str());
  }

  return SqliteStatement(db, fmt::to_string(stmt_buffer));
}
} // namespace

void TreeOverlayStore::saveTree(
    InodeNumber inodeNumber,
    const overlay::OverlayDir& odir) {
  db_->transaction([&](auto& txn) {
    // When `saveTree` gets called, caller is expected to rewrite the tree
    // content. So we need to remove the previously stored version.
    auto stmt =
        SqliteStatement(txn, "DELETE FROM ", kEntryTable, " WHERE parent = ?");
    stmt.bind(1, inodeNumber.get());
    stmt.step();

    // The following section generates the insertion SQLite statements based on
    // number of entries in `OverlayDir`. This is faster than inserting them
    // separately. Although we have to dynamically generate statements here.
    auto count = odir.entries_ref()->size();
    if (count == 0) {
      return;
    }

    size_t batch_count = count / kBatchInsertSize;
    auto remaining = count % kBatchInsertSize;
    auto entries_iter = odir.entries_ref()->cbegin();

    if (batch_count != 0) {
      auto batch_insert = makeBatchInsert(txn, kBatchInsertSize);
      for (size_t i = 0; i < batch_count; i++) {
        // One batch
        for (size_t n = 0; n < kBatchInsertSize; n++, entries_iter++) {
          auto name = PathComponentPiece{entries_iter->first};
          const auto& entry = entries_iter->second;
          insertInodeEntry(batch_insert, n, inodeNumber, name, entry);
        }

        batch_insert.step();
        batch_insert.reset();
      }
    }

    if (remaining != 0) {
      auto insert = makeBatchInsert(txn, remaining);
      for (size_t n = 0; entries_iter != odir.entries_ref()->cend();
           entries_iter++, n++) {
        auto name = PathComponentPiece{entries_iter->first};
        const auto& entry = entries_iter->second;
        insertInodeEntry(insert, n, inodeNumber, name, entry);
      }
      insert.step();
    }
  });
}

overlay::OverlayDir TreeOverlayStore::loadTree(InodeNumber inode) {
  overlay::OverlayDir dir;

  db_->transaction([&](auto& txn) {
    auto query = SqliteStatement(
        txn,
        "SELECT name, dtype, inode, hash FROM ",
        kEntryTable,
        " WHERE parent = ? ORDER BY name");
    query.bind(1, inode.get());

    while (query.step()) {
      auto name = query.columnBlob(0);
      overlay::OverlayEntry entry;
      entry.mode_ref() =
          dtype_to_mode(static_cast<dtype_t>(query.columnUint64(1)));
      entry.inodeNumber_ref() = query.columnUint64(2);
      entry.hash_ref() = query.columnBlob(3).toString();
      dir.entries_ref()->emplace(std::make_pair(name, entry));
    }
  });

  return dir;
}

void TreeOverlayStore::removeTree(InodeNumber inode) {
  db_->transaction([&](auto& txn) {
    auto children = SqliteStatement(
        txn, "SELECT COUNT(*) FROM ", kEntryTable, " WHERE parent = ?");
    children.bind(1, inode.get());

    if (!children.step() || children.columnUint64(0) != 0) {
      throw TreeOverlayNonEmptyError("cannot delete non-empty directory");
    }

    auto deleteInode =
        SqliteStatement(txn, "DELETE FROM ", kEntryTable, " WHERE parent = ?");
    deleteInode.bind(1, inode.get());
    deleteInode.step();
  });
}

bool TreeOverlayStore::hasTree(InodeNumber inode) {
  auto db = db_->lock();
  auto query =
      SqliteStatement(db, "SELECT 1 FROM ", kEntryTable, " WHERE parent = ?");
  query.bind(1, inode.get());
  if (query.step()) {
    return query.columnUint64(0) == 1;
  }
  return false;
}

void TreeOverlayStore::addChild(
    InodeNumber parent,
    PathComponentPiece name,
    overlay::OverlayEntry entry) {
  auto db = db_->lock();

  auto stmt = SqliteStatement(
      db,
      "INSERT INTO ",
      kEntryTable,
      " (parent, name, dtype, inode, sequence_id, hash) ",
      " VALUES (?, ?, ?, ?, ?, ?)");

  insertInodeEntry(stmt, 0, parent, name, entry);
  stmt.step();
}

void TreeOverlayStore::removeChild(
    InodeNumber parent,
    PathComponentPiece childName) {
  auto db = db_->lock();
  auto stmt = SqliteStatement(
      db, "DELETE FROM ", kEntryTable, " WHERE parent = ? AND name = ?");

  stmt.bind(1, parent.get());
  stmt.bind(2, childName.stringPiece());
  stmt.step();
}

void TreeOverlayStore::renameChild(
    InodeNumber src,
    InodeNumber dst,
    PathComponentPiece srcName,
    PathComponentPiece dstName) {
  // When rename also overwrites some file in the destination, we need to make
  // sure this is transactional.
  db_->transaction([&](auto& txn) {
    auto overwriteEmpty = SqliteStatement(
        txn,
        "SELECT COUNT(1) FROM ",
        kEntryTable,
        " WHERE `parent` = (SELECT `inode` FROM ",
        kEntryTable,
        " WHERE `parent` = ? AND `name` = ?)");
    overwriteEmpty.bind(1, dst.get());
    overwriteEmpty.bind(2, dstName.stringPiece());

    if (!(overwriteEmpty.step() && overwriteEmpty.columnUint64(0) == 0)) {
      throw TreeOverlayNonEmptyError("cannot overwrite non-empty directory");
    }

    // If all the check passes, we delete the child being overwritten
    auto deleteStmt = SqliteStatement(
        txn, "DELETE FROM ", kEntryTable, " WHERE parent = ? AND name = ?");
    deleteStmt.bind(1, dst.get());
    deleteStmt.bind(2, dstName.stringPiece());
    deleteStmt.step();

    auto stmt = SqliteStatement(
        txn,
        "UPDATE ",
        kEntryTable,
        " SET parent = ?, name = ? WHERE parent = ? AND name = ?");
    stmt.bind(1, dst.get());
    stmt.bind(2, dstName.stringPiece());
    stmt.bind(3, src.get());
    stmt.bind(4, srcName.stringPiece());
    stmt.step();
  });
}

void TreeOverlayStore::insertInodeEntry(
    SqliteStatement& inserts,
    size_t index,
    InodeNumber parent,
    PathComponentPiece name,
    const overlay::OverlayEntry& entry) {
  auto mode = static_cast<uint32_t>(entry.mode_ref().value());
  auto dtype = static_cast<uint32_t>(mode_to_dtype(mode));
  auto inode = entry.inodeNumber_ref().value();
  folly::ByteRange hash;

  if (auto entryHash = entry.hash_ref()) {
    hash = folly::ByteRange{
        reinterpret_cast<const unsigned char*>(entryHash->data()),
        entryHash->size()};
  }

  auto start = index * 6; // Number of columns
  inserts.bind(start + 1, parent.get());
  inserts.bind(start + 2, name.stringPiece());
  inserts.bind(start + 3, dtype);
  inserts.bind(start + 4, inode);
  inserts.bind(start + 5, nextEntryId_++);
  inserts.bind(start + 6, hash);
}
} // namespace facebook::eden
