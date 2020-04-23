/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "SqliteOverlay.h"

#include <folly/String.h>
#include <folly/container/Array.h>
#include <folly/logging/xlog.h>
#include <thrift/lib/cpp2/protocol/Serializer.h>
#include <iostream>
#include "eden/fs/fuse/InodeNumber.h"
#include "eden/fs/sqlite/Sqlite.h"
#include "eden/fs/store/StoreResult.h"
#include "eden/fs/utils/PathFuncs.h"

namespace facebook {
namespace eden {

using folly::ByteRange;
using folly::StringPiece;
using std::string;

constexpr folly::StringPiece kInodeTable = "Inode";
constexpr folly::StringPiece kConfigTable = "Config";
constexpr PathComponentPiece kOverlayName{"overlay.db"_pc};
constexpr uint32_t kNextInodeNumber = 1;
constexpr uint64_t kStartInodeNumber = 100;

// TODO: We should customize it by reading the value from the config. This is
// part of the stop gap solution put in place for recovery from unclean
// shutdown. More details in the header file.
constexpr uint64_t kInodeAllocationRange = 100;

SqliteOverlay::SqliteOverlay(AbsolutePathPiece localDir)
    : localDir_{std::move(localDir)} {}

SqliteOverlay::~SqliteOverlay() {
  if (db_) {
    db_->close();
  }
}

std::optional<InodeNumber> SqliteOverlay::initOverlay(
    bool createIfNonExisting) {
  if (createIfNonExisting) {
    ensureDirectoryExists(localDir_);
  }

  db_ = std::make_unique<SqliteDatabase>(localDir_ + kOverlayName);
  auto db = db_->lock();

  // Write ahead log for faster perf https://www.sqlite.org/wal.html
  SqliteStatement(db, "PRAGMA journal_mode=WAL").step();

  // The Inode table stores the information about each inode. At this point we
  // are only using it to store the information about the directory entries
  SqliteStatement(
      db,
      "CREATE TABLE IF NOT EXISTS ",
      kInodeTable,
      "(",
      "inode BIGINT NOT NULL,",
      "isdir INT NOT NULL,",
      "value BINARY NOT NULL,",
      "PRIMARY KEY (inode)",
      ")")
      .step();

  SqliteStatement(
      db,
      "CREATE TABLE IF NOT EXISTS ",
      kConfigTable,
      "(",
      "key INT NOT NULL,",
      "value BINARY NOT NULL,",
      "PRIMARY KEY (key)",
      ")")
      .step();

  // In the following code we read the last know used inode number and allocate
  // a range of inodes by saving the incremented value in db.
  uint64_t nextInodeNumber;
  auto optNextInodeNumber = readNextInodeNumber(db);
  if (optNextInodeNumber.has_value()) {
    nextInodeNumber = optNextInodeNumber.value();
  } else {
    // This will only be true if this is the first run.
    nextInodeNumber = kStartInodeNumber;
  }
  auto nextValue = nextInodeNumber + kStartInodeNumber;
  writeNextInodeNumber(db, nextValue);
  nextInodeNumber_.store(nextValue, std::memory_order_release);

  // The only reason we return an optional value is to have a common interface
  // with FsOverlay. This would change once we have implement OverlayChecker.
  return std::make_optional(InodeNumber{nextInodeNumber});
}

void SqliteOverlay::close(std::optional<InodeNumber> nextInodeNumber) {
  if (nextInodeNumber.has_value()) {
    saveNextInodeNumber(nextInodeNumber.value().get());
  }
  db_->close();
}

void SqliteOverlay::updateUsedInodeNumber(uint64_t usedInodeNumber) {
  saveNextInodeNumber(usedInodeNumber + 1);
}

std::optional<std::string> SqliteOverlay::load(uint64_t inodeNumber) const {
  auto db = db_->lock();

  SqliteStatement stmt(
      db, "select value from ", kInodeTable, " where inode = ?");

  // Bind the inode; parameters are 1-based
  stmt.bind(1, inodeNumber);

  if (stmt.step()) {
    // Return the result; columns are 0-based!
    auto blob = stmt.columnBlob(0);
    return std::string(blob.data(), blob.size());
  }

  // the inode does not exist
  return std::nullopt;
}

bool SqliteOverlay::hasInode(uint64_t inodeNumber) const {
  auto db = db_->lock();

  SqliteStatement stmt(db, "select 1 from ", kInodeTable, " where inode = ?");

  stmt.bind(1, inodeNumber);
  return stmt.step();
}

void SqliteOverlay::save(
    uint64_t inodeNumber,
    bool isDirectory,
    ByteRange value) {
  auto db = db_->lock();

  SqliteStatement stmt(
      db,
      // TODO: we need `or ignore` otherwise we hit primary key violations
      // when running our integration tests.  This implies that we're
      // over-fetching and that we have a perf improvement opportunity.
      "insert or replace into ",
      kInodeTable,
      " VALUES(?, ?, ?)");

  const uint32_t dir = isDirectory ? 1 : 0;

  stmt.bind(1, inodeNumber);
  stmt.bind(2, dir);
  stmt.bind(3, value);
  stmt.step();
}

void SqliteOverlay::saveOverlayDir(
    InodeNumber inodeNumber,
    const overlay::OverlayDir& odir) {
  // Ask thrift to serialize it.
  auto serializedData =
      apache::thrift::CompactSerializer::serialize<std::string>(odir);

  save(
      inodeNumber.getRawValue(),
      /*isDirectory=*/true,
      folly::StringPiece(serializedData));
}

std::optional<overlay::OverlayDir> SqliteOverlay::loadOverlayDir(
    InodeNumber inodeNumber) {
  auto serializedData = load(inodeNumber.getRawValue());
  if (!serializedData.has_value()) {
    return std::nullopt;
  }

  return apache::thrift::CompactSerializer::deserialize<overlay::OverlayDir>(
      serializedData.value());
}

void SqliteOverlay::removeOverlayData(InodeNumber inodeNumber) {
  auto db = db_->lock();

  SqliteStatement stmt(db, "delete from ", kInodeTable, " where inode = ?");

  stmt.bind(1, inodeNumber.get());
  stmt.step();
}

bool SqliteOverlay::hasOverlayData(InodeNumber inodeNumber) {
  return hasInode(inodeNumber.get());
}

void SqliteOverlay::saveNextInodeNumber(uint64_t inodeNumber) {
  if (inodeNumber >= nextInodeNumber_.load(std::memory_order_relaxed)) {
    auto db = db_->lock();

    // Check again in case some other thread won the race to acquire the lock
    if (inodeNumber >= nextInodeNumber_.load(std::memory_order_relaxed)) {
      auto nextValue = inodeNumber + kInodeAllocationRange;
      writeNextInodeNumber(db, nextValue);
      nextInodeNumber_.store(nextValue, std::memory_order_relaxed);
    }
  }
}

std::optional<uint64_t> SqliteOverlay::readNextInodeNumber(LockedDbPtr& db) {
  SqliteStatement stmt(
      db, "select value from ", kConfigTable, " where key = ?");

  // Bind the key; parameters are 1-based
  stmt.bind(1, kNextInodeNumber);

  if (stmt.step()) {
    // Return the result; columns are 0-based!
    auto blob = stmt.columnBlob(0);
    if (blob.size() != sizeof(uint64_t)) {
      throw std::logic_error(folly::sformat(
          "Unable to fetch the next inode number from the db, size: {}",
          blob.size()));
    }
    uint64_t ino;
    std::memcpy(&ino, blob.data(), sizeof(ino));
    return std::make_optional(ino);
  }
  return std::nullopt;
}

void SqliteOverlay::writeNextInodeNumber(
    LockedDbPtr& db,
    uint64_t inodeNumber) {
  folly::StringPiece ino{ByteRange(
      reinterpret_cast<const uint8_t*>(&inodeNumber),
      reinterpret_cast<const uint8_t*>(&inodeNumber + 1))};

  SqliteStatement stmt(
      db, "insert or replace into ", kConfigTable, " VALUES(?, ?)");

  stmt.bind(1, kNextInodeNumber);
  stmt.bind(2, ino);
  stmt.step();
}

} // namespace eden
} // namespace facebook
