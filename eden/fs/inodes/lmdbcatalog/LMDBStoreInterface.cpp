/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/lmdbcatalog/LMDBStoreInterface.h"

#include <folly/Range.h>
#include <thrift/lib/cpp2/protocol/Serializer.h>
#include <array>
#include <iterator>
#include "eden/fs/inodes/InodeNumber.h"
#include "eden/fs/inodes/overlay/gen-cpp2/overlay_types.h"
#include "eden/fs/lmdb/LMDBDatabase.h"
#include "eden/fs/telemetry/StructuredLogger.h"
#include "eden/fs/utils/DirType.h"

namespace facebook::eden {

using apache::thrift::CompactSerializer;

namespace {

// Initial Inode ID is root ID + 1
constexpr auto kInitialNodeId = kRootNodeId.getRawValue() + 1;

} // namespace

namespace {
std::unique_ptr<LMDBDatabase> removeAndRecreateDb(AbsolutePathPiece path) {
  int rc = ::unlink(path.copy().c_str());
  if (rc != 0 && errno != ENOENT) {
    throw_<std::runtime_error>(
        "Unable to remove lmdb database ", path, ", errno: ", errno);
  }
  return std::make_unique<LMDBDatabase>(path);
}

std::unique_ptr<LMDBDatabase> openAndVerifyDb(
    AbsolutePathPiece path,
    std::shared_ptr<StructuredLogger> /*logger*/) {
  try {
    return std::make_unique<LMDBDatabase>(path);
  } catch (const std::exception& ex) {
    if (folly::kIsWindows) {
      XLOG(WARN) << "LMDBDatabase (" << path
                 << ") failed to open: " << ex.what();
      return removeAndRecreateDb(path);
    }
    throw;
  }
}
} // namespace

LMDBStoreInterface::LMDBStoreInterface(
    AbsolutePathPiece path,
    std::shared_ptr<StructuredLogger> logger) {
  ensureDirectoryExists(path);

  db_ = openAndVerifyDb(path, std::move(logger));
}

LMDBStoreInterface::LMDBStoreInterface(std::unique_ptr<LMDBDatabase> db)
    : db_{std::move(db)} {}

void LMDBStoreInterface::close() {
  if (db_) {
    db_->close();
  }
}

std::unique_ptr<LMDBDatabase> LMDBStoreInterface::takeDatabase() {
  return std::move(db_);
}

InodeNumber LMDBStoreInterface::loadCounters() {
  // load next inode number
  uint64_t inode = 0;

  db_->transaction([&](LockedLMDBConnection& lockedConn) {
    MDB_cursor* cursor;
    checkLMDBResult(
        mdb_cursor_open(lockedConn->mdb_txn_, lockedConn->mdb_dbi_, &cursor));
    MDB_val mdb_key, mdb_value;
    auto result = mdb_cursor_get(cursor, &mdb_key, &mdb_value, MDB_FIRST);

    while (result == MDB_SUCCESS) {
      auto key = std::stoull(
          std::string{static_cast<char*>(mdb_key.mv_data), mdb_key.mv_size});
      if (key > inode) {
        inode = key;
      }

      try {
        auto odir =
            CompactSerializer::deserialize<overlay::OverlayDir>(std::string{
                static_cast<char*>(mdb_value.mv_data), mdb_value.mv_size});
        for (auto entries_iter = odir.entries_ref()->cbegin();
             entries_iter != odir.entries_ref()->cend();
             entries_iter++) {
          const auto& entry = entries_iter->second;

          auto entryIno =
              InodeNumber::fromThrift(*entry.inodeNumber_ref()).get();

          if (entryIno > inode) {
            inode = entryIno;
          }
        }
      } catch (std::exception&) {
        // If we can't deserialize, its likely a blob. Ignore the error
      }

      result = mdb_cursor_get(cursor, &mdb_key, &mdb_value, MDB_NEXT);
    }

    mdb_cursor_close(cursor);
  });

  if (inode == 0) {
    nextInode_ = kInitialNodeId;
  } else {
    nextInode_ = inode + 1;
  }

  return InodeNumber{nextInode_.load()};
}

InodeNumber LMDBStoreInterface::nextInodeNumber() {
  return InodeNumber{nextInode_.fetch_add(1, std::memory_order_acq_rel)};
}

std::vector<InodeNumber> LMDBStoreInterface::getAllParentInodeNumbers() {
  std::vector<InodeNumber> inodes;

  db_->transaction([&](LockedLMDBConnection& lockedConn) {
    MDB_cursor* cursor;
    checkLMDBResult(
        mdb_cursor_open(lockedConn->mdb_txn_, lockedConn->mdb_dbi_, &cursor));

    MDB_val mdb_key, mdb_value;
    auto result = mdb_cursor_get(cursor, &mdb_key, &mdb_value, MDB_FIRST);

    while (result == MDB_SUCCESS) {
      auto key = std::stoull(
          std::string{static_cast<char*>(mdb_key.mv_data), mdb_key.mv_size});
      inodes.push_back(InodeNumber{key});
      result = mdb_cursor_get(cursor, &mdb_key, &mdb_value, MDB_NEXT);
    }

    mdb_cursor_close(cursor);
  });

  return inodes;
}

void LMDBStoreInterface::saveBlob(
    InodeNumber inode,
    iovec* iov,
    size_t iovCount) {
  std::string key = std::to_string(inode.get());

  size_t size = 0;

  for (size_t i = 0; i < iovCount; i++) {
    size += iov[i].iov_len;
  }

  db_->transaction([&](LockedLMDBConnection& lockedConn) {
    MDB_val mdb_key, mdb_value;
    mdb_key.mv_data = const_cast<char*>(key.data());
    mdb_key.mv_size = key.size();
    mdb_value.mv_size = size;
    // Use MDB_RESERVE and memcpy here to avoid an extra copy
    checkLMDBResult(mdb_put(
        lockedConn->mdb_txn_,
        lockedConn->mdb_dbi_,
        &mdb_key,
        &mdb_value,
        MDB_RESERVE));
    char* value = (char*)mdb_value.mv_data;
    for (size_t i = 0; i < iovCount; i++) {
      memcpy(value, iov[i].iov_base, iov[i].iov_len);
      value += iov[i].iov_len;
    }
  });
}

void LMDBStoreInterface::saveTree(InodeNumber inode, std::string&& odir) {
  std::string key = std::to_string(inode.get());

  db_->transaction([&](LockedLMDBConnection& lockedConn) {
    MDB_val mdb_key, mdb_value;
    mdb_key.mv_data = const_cast<char*>(key.data());
    mdb_key.mv_size = key.size();
    mdb_value.mv_data = const_cast<char*>(odir.data());
    mdb_value.mv_size = odir.size();
    checkLMDBResult(mdb_put(
        lockedConn->mdb_txn_, lockedConn->mdb_dbi_, &mdb_key, &mdb_value, 0));
  });
}

std::string LMDBStoreInterface::loadBlob(InodeNumber inode) {
  std::string key = std::to_string(inode.get());
  std::string blob;

  db_->transaction([&](LockedLMDBConnection& lockedConn) {
    MDB_val mdb_key, mdb_value;
    mdb_key.mv_data = const_cast<char*>(key.data());
    mdb_key.mv_size = key.size();

    auto result = mdb_get(
        lockedConn->mdb_txn_, lockedConn->mdb_dbi_, &mdb_key, &mdb_value);

    if (result == MDB_SUCCESS) {
      blob.reserve(mdb_value.mv_size);
      blob.assign(static_cast<char*>(mdb_value.mv_data), mdb_value.mv_size);
    } else {
      checkLMDBResult(result);
      folly::assume_unreachable();
    }
  });

  return blob;
}

overlay::OverlayDir LMDBStoreInterface::loadTree(InodeNumber inode) {
  std::string key = std::to_string(inode.get());
  std::string tree;

  db_->transaction([&](LockedLMDBConnection& lockedConn) {
    MDB_val mdb_key, mdb_value;
    mdb_key.mv_data = const_cast<char*>(key.data());
    mdb_key.mv_size = key.size();

    auto result = mdb_get(
        lockedConn->mdb_txn_, lockedConn->mdb_dbi_, &mdb_key, &mdb_value);

    if (result == MDB_SUCCESS) {
      tree.reserve(mdb_value.mv_size);
      tree.assign(static_cast<char*>(mdb_value.mv_data), mdb_value.mv_size);
    } else if (result != MDB_NOTFOUND) {
      // if the inode is not found, we are expected to just return an empty
      // OverlayDir
      checkLMDBResult(result);
      folly::assume_unreachable();
    }
  });

  if (tree.empty()) {
    return overlay::OverlayDir{};
  }
  return CompactSerializer::deserialize<overlay::OverlayDir>(tree);
}

overlay::OverlayDir LMDBStoreInterface::loadAndRemoveTree(InodeNumber inode) {
  std::string key = std::to_string(inode.get());
  std::string tree;

  db_->transaction([&](LockedLMDBConnection& lockedConn) {
    MDB_val mdb_key, mdb_value;
    mdb_key.mv_data = const_cast<char*>(key.data());
    mdb_key.mv_size = key.size();

    auto result = mdb_get(
        lockedConn->mdb_txn_, lockedConn->mdb_dbi_, &mdb_key, &mdb_value);

    if (result == MDB_SUCCESS) {
      tree.reserve(mdb_value.mv_size);
      tree.assign(static_cast<char*>(mdb_value.mv_data), mdb_value.mv_size);

      result = mdb_del(
          lockedConn->mdb_txn_, lockedConn->mdb_dbi_, &mdb_key, nullptr);

      if (result != MDB_SUCCESS && result != MDB_NOTFOUND) {
        // don't inode not found as a fatal
        checkLMDBResult(result);
        folly::assume_unreachable();
      }
    } else if (result != MDB_NOTFOUND) {
      // if the inode is not found, we are expected to just return an empty
      // OverlayDir
      checkLMDBResult(result);
      folly::assume_unreachable();
    }
  });

  if (tree.empty()) {
    return overlay::OverlayDir{};
  }
  return CompactSerializer::deserialize<overlay::OverlayDir>(tree);
}

void LMDBStoreInterface::removeBlob(InodeNumber inode) {
  removeData(inode);
}

void LMDBStoreInterface::removeTree(InodeNumber inode) {
  removeData(inode);
}

void LMDBStoreInterface::removeData(InodeNumber inode) {
  std::string key = std::to_string(inode.get());

  db_->transaction([&](LockedLMDBConnection& lockedConn) {
    MDB_val mdb_key;
    mdb_key.mv_data = const_cast<char*>(key.data());
    mdb_key.mv_size = key.size();
    auto result =
        mdb_del(lockedConn->mdb_txn_, lockedConn->mdb_dbi_, &mdb_key, nullptr);

    if (result != MDB_SUCCESS && result != MDB_NOTFOUND) {
      // don't treat inode not found as a fatal
      checkLMDBResult(result);
      folly::assume_unreachable();
    }
  });
}

bool LMDBStoreInterface::hasBlob(InodeNumber inode) {
  return hasData(inode);
}

bool LMDBStoreInterface::hasTree(InodeNumber inode) {
  return hasData(inode);
}

bool LMDBStoreInterface::hasData(InodeNumber inode) {
  std::string key = std::to_string(inode.get());
  bool treeExists;

  db_->transaction([&](LockedLMDBConnection& lockedConn) {
    MDB_val mdb_key, mdb_value;
    mdb_key.mv_data = const_cast<char*>(key.data());
    mdb_key.mv_size = key.size();

    auto result = mdb_get(
        lockedConn->mdb_txn_, lockedConn->mdb_dbi_, &mdb_key, &mdb_value);

    if (result == MDB_SUCCESS) {
      treeExists = true;
    } else if (result == MDB_NOTFOUND) {
      treeExists = false;
    } else {
      checkLMDBResult(result);
      folly::assume_unreachable();
    }
  });

  return treeExists;
}

FileOffset LMDBStoreInterface::allocateBlob(
    InodeNumber inode,
    FileOffset offset,
    FileOffset length) {
  std::string key = std::to_string(inode.get());
  std::string blob;
  FileOffset ret = 0;

  db_->transaction([&](LockedLMDBConnection& lockedConn) {
    MDB_val mdb_key, mdb_value;
    mdb_key.mv_data = const_cast<char*>(key.data());
    mdb_key.mv_size = key.size();

    auto result = mdb_get(
        lockedConn->mdb_txn_, lockedConn->mdb_dbi_, &mdb_key, &mdb_value);
    if (result == MDB_SUCCESS) {
      if (offset + length > (FileOffset)mdb_value.mv_size) {
        blob.reserve(offset + length);
        auto oldSize = (FileOffset)mdb_value.mv_size;
        blob.assign(static_cast<char*>(mdb_value.mv_data), mdb_value.mv_size);

        auto replaceStart = oldSize;
        auto replaceLength = (offset + length - oldSize);
        // Replace with null characters as per fallocate definition
        blob.replace(replaceStart, replaceLength, replaceLength, '\0');

        mdb_value.mv_data = const_cast<char*>(blob.data());
        mdb_value.mv_size = blob.size();

        result = mdb_put(
            lockedConn->mdb_txn_,
            lockedConn->mdb_dbi_,
            &mdb_key,
            &mdb_value,
            0);
        if (result != MDB_SUCCESS) {
          ret = -1;
          logLMDBError(result);
        }
      }
    } else {
      ret = -1;
      logLMDBError(result);
    }
  });
  return ret;
}

FileOffset LMDBStoreInterface::pwriteBlob(
    InodeNumber inode,
    const struct iovec* iov,
    int iovcnt,
    FileOffset offset) {
  std::string key = std::to_string(inode.get());
  std::string blobToWrite;
  std::string blob;
  size_t size = 0;

  for (size_t i = 0; i < (size_t)iovcnt; i++) {
    size += iov[i].iov_len;
  }

  blobToWrite.resize(size);

  // Copy the data from the iovecs into the string
  char* dest = blobToWrite.data();
  for (size_t i = 0; i < (size_t)iovcnt; i++) {
    std::memcpy(dest, iov[i].iov_base, iov[i].iov_len);
    dest += iov[i].iov_len;
  }

  db_->transaction([&](LockedLMDBConnection& lockedConn) {
    MDB_val mdb_key, mdb_value;
    mdb_key.mv_data = const_cast<char*>(key.data());
    mdb_key.mv_size = key.size();

    auto result = mdb_get(
        lockedConn->mdb_txn_, lockedConn->mdb_dbi_, &mdb_key, &mdb_value);
    if (result == MDB_SUCCESS) {
      blob.reserve(mdb_value.mv_size);
      blob.assign(static_cast<char*>(mdb_value.mv_data), mdb_value.mv_size);

      if (offset + size > mdb_value.mv_size) {
        blob.resize(offset + size);
      }
      blob.replace(offset, size, blobToWrite);

      mdb_value.mv_data = const_cast<char*>(blob.data());
      mdb_value.mv_size = blob.size();

      result = mdb_put(
          lockedConn->mdb_txn_, lockedConn->mdb_dbi_, &mdb_key, &mdb_value, 0);
      if (result != MDB_SUCCESS) {
        size = static_cast<size_t>(-1);
        logLMDBError(result);
      }
    } else {
      size = static_cast<size_t>(-1);
      logLMDBError(result);
    }
  });
  return size;
}

FileOffset LMDBStoreInterface::preadBlob(
    InodeNumber inode,
    void* buf,
    size_t n,
    FileOffset offset) {
  std::string key = std::to_string(inode.get());
  FileOffset ret;

  db_->transaction([&](LockedLMDBConnection& lockedConn) {
    MDB_val mdb_key, mdb_value;
    mdb_key.mv_data = const_cast<char*>(key.data());
    mdb_key.mv_size = key.size();

    auto result = mdb_get(
        lockedConn->mdb_txn_, lockedConn->mdb_dbi_, &mdb_key, &mdb_value);

    if (result == MDB_SUCCESS) {
      if (offset + n > mdb_value.mv_size) {
        n = mdb_value.mv_size - offset;
      }
      void* copyStart = static_cast<char*>(mdb_value.mv_data) + offset;
      memcpy(buf, copyStart, n);
      ret = n;
    } else {
      ret = -1;
      logLMDBError(result);
    }
  });
  return ret;
}

FileOffset LMDBStoreInterface::getBlobSize(InodeNumber inode) {
  std::string key = std::to_string(inode.get());
  FileOffset size;

  db_->transaction([&](LockedLMDBConnection& lockedConn) {
    MDB_val mdb_key, mdb_value;
    mdb_key.mv_data = const_cast<char*>(key.data());
    mdb_key.mv_size = key.size();

    auto result = mdb_get(
        lockedConn->mdb_txn_, lockedConn->mdb_dbi_, &mdb_key, &mdb_value);

    if (result == MDB_SUCCESS) {
      size = (FileOffset)mdb_value.mv_size;
    } else {
      size = -1;
      logLMDBError(result);
    }
  });
  return size;
}

FileOffset LMDBStoreInterface::truncateBlob(
    InodeNumber inode,
    FileOffset length) {
  std::string key = std::to_string(inode.get());
  std::string blob;
  FileOffset ret = 0;

  db_->transaction([&](LockedLMDBConnection& lockedConn) {
    MDB_val mdb_key, mdb_value;
    mdb_key.mv_data = const_cast<char*>(key.data());
    mdb_key.mv_size = key.size();

    auto result = mdb_get(
        lockedConn->mdb_txn_, lockedConn->mdb_dbi_, &mdb_key, &mdb_value);

    if (result == MDB_SUCCESS) {
      if (length > (FileOffset)mdb_value.mv_size) {
        blob.reserve(mdb_value.mv_size);
        blob.assign(static_cast<char*>(mdb_value.mv_data), mdb_value.mv_size);
        blob.resize(length);

        mdb_value.mv_data = const_cast<char*>(blob.data());
        mdb_value.mv_size = blob.size();
      } else {
        mdb_value.mv_size = length;
      }
      result = mdb_put(
          lockedConn->mdb_txn_, lockedConn->mdb_dbi_, &mdb_key, &mdb_value, 0);
      if (result != MDB_SUCCESS) {
        ret = -1;
        logLMDBError(result);
      }
    } else {
      ret = -1;
      logLMDBError(result);
    }
  });
  return ret;
}

} // namespace facebook::eden
