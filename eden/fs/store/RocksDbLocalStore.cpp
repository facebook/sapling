/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/RocksDbLocalStore.h"

#include <array>
#include <atomic>

#include <fb303/ServiceData.h>
#include <folly/String.h>
#include <folly/futures/Future.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <folly/lang/Bits.h>
#include <folly/logging/xlog.h>
#include <rocksdb/convenience.h>
#include <rocksdb/db.h>
#include <rocksdb/filter_policy.h>
#include <rocksdb/table.h>

#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/rocksdb/RocksException.h"
#include "eden/fs/rocksdb/RocksHandles.h"
#include "eden/fs/store/KeySpace.h"
#include "eden/fs/store/StoreResult.h"
#include "eden/fs/telemetry/StructuredLogger.h"
#include "eden/fs/utils/Bug.h"
#include "eden/fs/utils/FaultInjector.h"
#include "eden/fs/utils/Throw.h"

using folly::ByteRange;
using folly::Synchronized;
using rocksdb::ReadOptions;
using rocksdb::Slice;
using rocksdb::SliceParts;
using rocksdb::WriteOptions;
using std::string;
using std::chrono::duration_cast;

namespace {
using namespace facebook::eden;

rocksdb::ColumnFamilyOptions makeColumnOptions(uint64_t LRUblockCacheSizeMB) {
  rocksdb::ColumnFamilyOptions options;

  // We'll never perform range scans on any of the keys that we store.
  // This enables bloom filters and a hash policy that improves our
  // get/put performance.
  options.OptimizeForPointLookup(LRUblockCacheSizeMB);

  options.OptimizeLevelStyleCompaction();
  return options;
}

/**
 * The different key spaces that we desire.
 * The ordering is coupled with the values of the KeySpace enum.
 */
const std::vector<rocksdb::ColumnFamilyDescriptor> columnFamilies(
    const rocksdb::DBOptions& db_options,
    const std::string& name) {
  // Most of the column families will share the same cache.  We
  // want the blob data to live in its own smaller cache; the assumption
  // is that the vfs cache will compensate for that, together with the
  // idea that we shouldn't need to materialize a great many files.
  auto options = makeColumnOptions(64);
  auto blobOptions = makeColumnOptions(8);

  // We have to open all column families that currenly exists in our RocksDb.
  // Else we will get "Invalid argument: You have to open all column
  // families." when we try to open the DB. This tracks if there are any
  // pre-existing column families that we don't open (may be the cause if we
  // delete a column family from KeySpace or need to roll back from a version
  // that added a column family.)
  std::vector<std::string> oldUnopenedColumnFamilies{};
  rocksdb::DB::ListColumnFamilies(db_options, name, &oldUnopenedColumnFamilies);

  std::vector<rocksdb::ColumnFamilyDescriptor> families;
  for (auto& ks : KeySpace::kAll) {
    families.emplace_back(
        ks->name.str(),
        (ks->index == KeySpace::BlobFamily.index) ? blobOptions : options);
    auto oldFamily = find(
        oldUnopenedColumnFamilies.begin(),
        oldUnopenedColumnFamilies.end(),
        ks->name.str());
    if (oldFamily != oldUnopenedColumnFamilies.end()) {
      oldUnopenedColumnFamilies.erase(oldFamily);
    }
  }
  // Put the default column family after the defined KeySpace values.
  // This way the KeySpace enum values can be used directly as indexes
  // into our column family vectors.
  families.emplace_back(rocksdb::kDefaultColumnFamilyName, options);
  auto oldFamily = find(
      oldUnopenedColumnFamilies.begin(),
      oldUnopenedColumnFamilies.end(),
      rocksdb::kDefaultColumnFamilyName);
  if (oldFamily != oldUnopenedColumnFamilies.end()) {
    oldUnopenedColumnFamilies.erase(oldFamily);
  }

  // add any column families we missed with our default options;
  // we have to open them but otherwise we don't care about them
  for (auto& family : oldUnopenedColumnFamilies) {
    families.emplace_back(family, options);
  }

  return families;
}

/**
 * Return a rocksdb::Range that contains all possible keys that we store.
 *
 * The input string will be used to store data for the Range slices.
 * The caller must ensure that the rangeStorage parameter remains valid and
 * unmodified until they are done using the returned Range.
 */
rocksdb::Range getFullRange(std::string& rangeStorage) {
  // An empty slice is the lowest possible value.
  Slice begin;
  // All of our keys are currently 20 bytes.
  // Use a longer key to ensure that this is greater than any valid key.
  rangeStorage = std::string(
      21, static_cast<char>(std::numeric_limits<unsigned char>::max()));
  Slice end(rangeStorage);
  return rocksdb::Range(begin, end);
}

rocksdb::Slice _createSlice(folly::ByteRange bytes) {
  return Slice(reinterpret_cast<const char*>(bytes.data()), bytes.size());
}

class RocksDbWriteBatch : public LocalStore::WriteBatch {
 public:
  void put(KeySpace keySpace, folly::ByteRange key, folly::ByteRange value)
      override;
  void put(
      KeySpace keySpace,
      folly::ByteRange key,
      std::vector<folly::ByteRange> valueSlices) override;
  void flush() override;
  ~RocksDbWriteBatch() override;
  // Use LocalStore::beginWrite() to create a write batch
  RocksDbWriteBatch(
      Synchronized<RocksDbLocalStore::RockDBState>::ConstRLockedPtr&& dbHandles,
      size_t bufferSize);

  void flushIfNeeded();

  folly::Synchronized<RocksDbLocalStore::RockDBState>::ConstRLockedPtr
      lockedDB_;
  rocksdb::WriteBatch writeBatch_;
  size_t bufSize_;
};

void RocksDbWriteBatch::flush() {
  auto pending = writeBatch_.Count();
  if (pending == 0) {
    return;
  }

  XLOG(DBG5) << "Flushing " << pending << " entries with data size of "
             << writeBatch_.GetDataSize();

  auto status = lockedDB_->handles->db->Write(WriteOptions(), &writeBatch_);
  XLOG(DBG5) << "... Flushed";

  if (!status.ok()) {
    throw RocksException::build(
        status, "error putting blob batch in local store");
  }

  writeBatch_.Clear();
}

void RocksDbWriteBatch::flushIfNeeded() {
  auto needFlush = bufSize_ > 0 && writeBatch_.GetDataSize() >= bufSize_;

  if (needFlush) {
    flush();
  }
}

RocksDbWriteBatch::RocksDbWriteBatch(
    Synchronized<RocksDbLocalStore::RockDBState>::ConstRLockedPtr&& dbHandles,
    size_t bufSize)
    : LocalStore::WriteBatch(),
      lockedDB_(std::move(dbHandles)),
      writeBatch_(bufSize),
      bufSize_(bufSize) {}

RocksDbWriteBatch::~RocksDbWriteBatch() {
  if (writeBatch_.Count() > 0) {
    XLOG(ERR) << "WriteBatch being destroyed with " << writeBatch_.Count()
              << " items pending flush";
  }
}

void RocksDbWriteBatch::put(
    KeySpace keySpace,
    folly::ByteRange key,
    folly::ByteRange value) {
  writeBatch_.Put(
      lockedDB_->handles->columns[keySpace->index].get(),
      _createSlice(key),
      _createSlice(value));

  flushIfNeeded();
}

void RocksDbWriteBatch::put(
    KeySpace keySpace,
    folly::ByteRange key,
    std::vector<folly::ByteRange> valueSlices) {
  std::vector<Slice> slices;

  for (auto& valueSlice : valueSlices) {
    slices.emplace_back(_createSlice(valueSlice));
  }

  auto keySlice = _createSlice(key);
  SliceParts keyParts(&keySlice, 1);
  writeBatch_.Put(
      lockedDB_->handles->columns[keySpace->index].get(),
      keyParts,
      SliceParts(slices.data(), slices.size()));

  flushIfNeeded();
}

rocksdb::Options getRocksdbOptions() {
  rocksdb::Options options;
  // Optimize RocksDB. This is the easiest way to get RocksDB to perform well.
  options.IncreaseParallelism();

  // Create the DB if it's not already present.
  options.create_if_missing = true;
  // Automatically create column families as we define new ones.
  options.create_missing_column_families = true;

  // Make sure we never hold more than 128MB onto the WAL
  options.max_total_wal_size = 128 * 1024 * 1024;

  return options;
}

RocksHandles openDB(AbsolutePathPiece path, RocksDBOpenMode mode) {
  auto options = getRocksdbOptions();
  const auto columnDescriptors =
      columnFamilies(rocksdb::DBOptions{options}, path.stringWithoutUNC());
  try {
    return RocksHandles(
        path.viewWithoutUNC(), mode, options, columnDescriptors);
  } catch (const RocksException& ex) {
    XLOG(ERR) << "Error opening RocksDB storage at " << path << ": "
              << ex.what();
    if (mode == RocksDBOpenMode::ReadOnly) {
      // In read-only mode fail rather than attempting to repair the DB.
      throw;
    }
    // Fall through and attempt to repair the DB
  }

  RocksDbLocalStore::repairDB(path);

  // Now try opening the DB again.
  return RocksHandles(path.viewWithoutUNC(), mode, options, columnDescriptors);
}

} // namespace

namespace facebook::eden {

RocksDbLocalStore::RockDBState::RockDBState() {
  XLOG(DBG2) << "Making a new RockDB localstore ( " << this
             << " ). debug information for T136469251.";
}

RocksDbLocalStore::RocksDbLocalStore(
    AbsolutePathPiece pathToRocksDb,
    std::shared_ptr<StructuredLogger> structuredLogger,
    FaultInjector* faultInjector,
    RocksDBOpenMode mode)
    : structuredLogger_{std::move(structuredLogger)},
      faultInjector_(*faultInjector),
      ioPool_(12, "RocksLocalStore"),
      pathToDb_{pathToRocksDb.copy()},
      mode_{mode} {
  XLOG(DBG2) << "Making a new RockDB localstore ( " << this
             << " ) . debug information for T136469251.";
}

void RocksDbLocalStore::open() {
  XLOG(DBG2) << "Opening Rocksdb localstore ( " << this
             << " ) . debug information for T136469251.";
  {
    auto handles = dbHandles_.wlock();
    switch (handles->status) {
      case RockDbHandleStatus::CLOSED:
        throw std::runtime_error(
            "Not opening the RocksDb store because it has already been closed.");
      case RockDbHandleStatus::OPEN:
        throw std::runtime_error(
            "Not opening the RocksDb store because it has already been opened.");
      case RockDbHandleStatus::NOT_YET_OPENED:
        break;
    }
    handles->handles =
        std::make_unique<RocksHandles>(openDB(pathToDb_.piece(), mode_));
    handles->status = RockDbHandleStatus::OPEN;
  }
  // Publish fb303 stats once when we first open the DB.
  // These will be kept up-to-date later by the periodicManagementTask() call.
  XLOG(DBG2) << "RocksDB opened, computing statistics ...";
  computeStats(/*publish=*/true, /*config=*/nullptr);
  XLOG(DBG2) << "RocksDB opened, clearing out old data ...";
  clearDeprecatedKeySpaces();
  XLOG(DBG2) << "RocksDB setup complete. ( " << this
             << " ) . debug information for T136469251.";
}

RocksDbLocalStore::~RocksDbLocalStore() {
  close();
}

void RocksDbLocalStore::close() {
  // Acquire dbHandles_ in write-lock mode.
  // Since any other access to the DB acquires a read lock this will block until
  // all current DB operations are complete.
  auto handles = dbHandles_.wlock();
  if (handles->status == RockDbHandleStatus::OPEN) {
    handles->handles->close();
    handles->handles = nullptr;
  }
  handles->status = RockDbHandleStatus::CLOSED;
  XLOG(DBG2) << "Closing Rocksdb localstore ( " << this
             << " ) . debug information for T136469251.";
}

folly::Synchronized<RocksDbLocalStore::RockDBState>::ConstRLockedPtr
RocksDbLocalStore::getHandles() const {
  auto handles = dbHandles_.rlock();
  switch (handles->status) {
    case RockDbHandleStatus::NOT_YET_OPENED:
      throwStoreNotYetOpenedError(handles);
    case RockDbHandleStatus::OPEN:
      if (!handles->handles->db) {
        EDEN_BUG()
            << "RockDB should be open, but the handles to the DB are invalid.";
      }
      break;
    case RockDbHandleStatus::CLOSED:
      throwStoreClosedError(handles);
  }

  return handles;
}

void RocksDbLocalStore::repairDB(AbsolutePathPiece path) {
  XLOG(ERR) << "Attempting to repair RocksDB " << path;
  rocksdb::ColumnFamilyOptions unknownColumFamilyOptions;
  unknownColumFamilyOptions.OptimizeForPointLookup(8);
  unknownColumFamilyOptions.OptimizeLevelStyleCompaction();

  auto dbPathStr = path.stringWithoutUNC();
  rocksdb::DBOptions dbOptions(getRocksdbOptions());

  const auto columnDescriptors =
      columnFamilies(dbOptions, path.stringWithoutUNC());

  auto status = RepairDB(
      dbPathStr, dbOptions, columnDescriptors, unknownColumFamilyOptions);
  if (!status.ok()) {
    throw RocksException::build(status, "unable to repair RocksDB at ", path);
  }
}

void RocksDbLocalStore::clearKeySpace(KeySpace keySpace) {
  auto handlesLock = getHandles();
  auto& handles = handlesLock->handles;
  auto columnFamily = handles->columns[keySpace->index].get();
  std::unique_ptr<rocksdb::Iterator> it{
      handles->db->NewIterator(ReadOptions(), columnFamily)};
  XLOG(DBG2) << "clearing column family \"" << columnFamily->GetName() << "\"";
  std::string rangeStorage;
  const auto fullRange = getFullRange(rangeStorage);

  // Delete all SST files that only contain keys in the specified range.
  // Since we are deleting everything in this column family this should
  // effectively delete everything.
  auto status = DeleteFilesInRange(
      handles->db.get(), columnFamily, &fullRange.start, &fullRange.limit);
  if (!status.ok()) {
    throw RocksException::build(
        status,
        "error deleting data in \"",
        columnFamily->GetName(),
        "\" column family");
  }

  // Call DeleteRange() as well.  In theory DeleteFilesInRange may not delete
  // everything in the range (but it probably will in our case since we are
  // intending to delete everything).
  const WriteOptions writeOptions;
  status = handles->db->DeleteRange(
      writeOptions, columnFamily, fullRange.start, fullRange.limit);
  if (!status.ok()) {
    throw RocksException::build(
        status,
        "error deleting data in \"",
        columnFamily->GetName(),
        "\" column family");
  }
}

void RocksDbLocalStore::compactKeySpace(KeySpace keySpace) {
  auto handlesLock = getHandles();
  auto& handles = handlesLock->handles;
  auto options = rocksdb::CompactRangeOptions{};
  options.allow_write_stall = true;
  auto columnFamily = handles->columns[keySpace->index].get();
  XLOG(DBG2) << "compacting column family \"" << columnFamily->GetName()
             << "\"";
  auto status = handles->db->CompactRange(
      options, columnFamily, /*begin=*/nullptr, /*end=*/nullptr);
  if (!status.ok()) {
    throw RocksException::build(
        status,
        "error compacting \"",
        columnFamily->GetName(),
        "\" column family");
  }
}

StoreResult RocksDbLocalStore::get(KeySpace keySpace, ByteRange key) const {
  auto handlesLock = getHandles();
  auto& handles = handlesLock->handles;
  string value;
  auto status = handles->db->Get(
      ReadOptions(),
      handles->columns[keySpace->index].get(),
      _createSlice(key),
      &value);
  if (!status.ok()) {
    if (status.IsNotFound()) {
      // Return an empty StoreResult
      return StoreResult::missing(keySpace, key);
    }

    // TODO: RocksDB can return a "TryAgain" error.
    // Should we try again for the user, rather than re-throwing the error?

    // We don't use RocksException::check(), since we don't want to waste our
    // time computing the hex string of the key if we succeeded.
    throw RocksException::build(
        status, "failed to get ", folly::hexlify(key), " from local store");
  }
  return StoreResult(std::move(value));
}

FOLLY_NODISCARD folly::Future<std::vector<StoreResult>>
RocksDbLocalStore::getBatch(
    KeySpace keySpace,
    const std::vector<folly::ByteRange>& keys) const {
  std::vector<folly::Future<std::vector<StoreResult>>> futures;

  std::vector<std::shared_ptr<std::vector<std::string>>> batches;
  batches.emplace_back(std::make_shared<std::vector<std::string>>());

  for (auto& key : keys) {
    if (batches.back()->size() >= 2048) {
      batches.emplace_back(std::make_shared<std::vector<std::string>>());
    }
    batches.back()->emplace_back(
        reinterpret_cast<const char*>(key.data()), key.size());
  }

  for (auto& batch : batches) {
    futures.emplace_back(
        faultInjector_.checkAsync("local store get batch", "")
            .semi()
            .via(&ioPool_)
            .thenValue([store = getSharedFromThis(),
                        keySpace,
                        keys = std::move(batch)](folly::Unit&&) {
              XLOG(DBG3) << __func__ << " starting to actually do work";
              auto handlesLock = store->getHandles();
              auto& handles = handlesLock->handles;
              std::vector<Slice> keySlices;
              std::vector<std::string> values;
              std::vector<rocksdb::ColumnFamilyHandle*> columns;
              for (auto& key : *keys) {
                keySlices.emplace_back(key);
                columns.emplace_back(handles->columns[keySpace->index].get());
              }
              auto statuses = handles->db->MultiGet(
                  ReadOptions(), columns, keySlices, &values);

              std::vector<StoreResult> results;
              for (size_t i = 0; i < keys->size(); ++i) {
                auto& status = statuses[i];
                if (!status.ok()) {
                  if (status.IsNotFound()) {
                    // Return an empty StoreResult
                    results.push_back(StoreResult::missing(
                        keySpace,
                        folly::ByteRange{folly::StringPiece{keys->at(i)}}));
                    continue;
                  }

                  // TODO: RocksDB can return a "TryAgain" error.
                  // Should we try again for the user, rather than
                  // re-throwing the error?

                  // We don't use RocksException::check(), since we don't
                  // want to waste our time computing the hex string of the
                  // key if we succeeded.
                  throw RocksException::build(
                      status,
                      "failed to get ",
                      folly::hexlify(keys->at(i)),
                      " from local store");
                }
                results.emplace_back(std::move(values[i]));
              }
              return results;
            }));
  }

  return folly::collectUnsafe(futures).thenValue(
      [](std::vector<std::vector<StoreResult>>&& tries) {
        std::vector<StoreResult> results;
        for (auto& batch : tries) {
          results.insert(
              results.end(),
              make_move_iterator(batch.begin()),
              make_move_iterator(batch.end()));
        }
        return results;
      });
}

bool RocksDbLocalStore::hasKey(KeySpace keySpace, folly::ByteRange key) const {
  string value;
  auto handlesLock = getHandles();
  auto& handles = handlesLock->handles;
  auto status = handles->db->Get(
      ReadOptions(),
      handles->columns[keySpace->index].get(),
      _createSlice(key),
      &value);
  if (!status.ok()) {
    if (status.IsNotFound()) {
      return false;
    }

    // TODO: RocksDB can return a "TryAgain" error.
    // Should we try again for the user, rather than re-throwing the error?

    // We don't use RocksException::check(), since we don't want to waste our
    // time computing the hex string of the key if we succeeded.
    throw RocksException::build(
        status, "failed to get ", folly::hexlify(key), " from local store");
  }
  return true;
}

std::unique_ptr<LocalStore::WriteBatch> RocksDbLocalStore::beginWrite(
    size_t bufSize) {
  return std::make_unique<RocksDbWriteBatch>(getHandles(), bufSize);
}

void RocksDbLocalStore::put(
    KeySpace keySpace,
    folly::ByteRange key,
    folly::ByteRange value) {
  auto handlesLock = getHandles();
  auto& handles = handlesLock->handles;
  handles->db->Put(
      WriteOptions(),
      handles->columns[keySpace->index].get(),
      _createSlice(key),
      _createSlice(value));
}

uint64_t RocksDbLocalStore::getApproximateSize(KeySpace keySpace) const {
  auto handlesLock = getHandles();
  auto& handles = handlesLock->handles;
  uint64_t size = 0;

  // kLiveSstFilesSize reports the size of all "live" sst files.
  // This excludes sst files from older snapshot versions that RocksDB may
  // still be holding onto.  e.g., to provide a consistent view to iterators.
  // kTotalSstFilesSize would report the size of all sst files if we wanted to
  // report that.
  uint64_t sstFilesSize;
  auto result = handles->db->GetIntProperty(
      handles->columns[keySpace->index].get(),
      rocksdb::DB::Properties::kLiveSstFilesSize,
      &sstFilesSize);
  if (result) {
    size += sstFilesSize;
  } else {
    XLOG(WARN) << "unable to retrieve SST file size from RocksDB for key space "
               << handles->columns[keySpace->index]->GetName();
  }

  // kSizeAllMemTables reports the size of the memtables.
  // This is the in-memory space for tracking the data in *.log files that have
  // not yet been compacted into a .sst file.
  //
  // We use this as a something that will hopefully roughly approximate the size
  // of the *.log files.  In practice this generally seems to be a fair amount
  // smaller than the on-disk *.log file size, except immediately after a
  // compaction when there is still a couple MB of in-memory metadata despite
  // having no uncompacted on-disk data.
  uint64_t memtableSize;
  result = handles->db->GetIntProperty(
      handles->columns[keySpace->index].get(),
      rocksdb::DB::Properties::kSizeAllMemTables,
      &memtableSize);
  if (result) {
    size += memtableSize;
  } else {
    XLOG(WARN) << "unable to retrieve memtable size from RocksDB for key space "
               << handles->columns[keySpace->index]->GetName();
  }

  return size;
}

void RocksDbLocalStore::periodicManagementTask(const EdenConfig& config) {
  enableBlobCaching.store(
      config.enableBlobCaching.getValue(), std::memory_order_relaxed);

  // Compute and publish the stats
  auto before = computeStats(/*publish=*/true, &config);

  // If any ephemeral column's size is more than its configured limit,
  // trigger garbage collection.
  if (before.excessiveKeySpaces.any()) {
    std::string keySpaceNames;
    for (auto& ks : KeySpace::kAll) {
      if (before.excessiveKeySpaces.test(ks->index)) {
        if (!keySpaceNames.empty()) {
          keySpaceNames.push_back(',');
        }
        keySpaceNames.append(ks->name.str());
      }
    }
    XLOG(INFO) << "scheduling automatic local store garbage collection: "
               << "ephemeral data sizes of columns " << keySpaceNames
               << " exceed their limits; total ephemeral size = "
               << before.ephemeral;
    triggerAutoGC(before);
  }
}

RocksDbLocalStore::SizeSummary RocksDbLocalStore::computeStats(
    bool publish,
    const EdenConfig* config) {
  SizeSummary result;
  for (const auto& ks : KeySpace::kAll) {
    auto size = getApproximateSize(ks);
    if (publish) {
      fb303::fbData->setCounter(
          folly::to<string>(statsPrefix_, ks->name, ".size"), size);
    }
    if (auto* ephemeral = std::get_if<Ephemeral>(&ks->persistence)) {
      result.ephemeral += size;
      if (config) {
        if (size > (config->*(ephemeral->cacheLimit)).getValue()) {
          result.excessiveKeySpaces.set(ks->index);
        }
      }
    } else if (!ks->isDeprecated()) {
      result.persistent += size;
    }
  }

  if (publish) {
    fb303::fbData->setCounter(
        folly::to<string>(statsPrefix_, "ephemeral.total_size"),
        result.ephemeral);
    fb303::fbData->setCounter(
        folly::to<string>(statsPrefix_, "persistent.total_size"),
        result.persistent);
  }

  return result;
}

// In the future it would perhaps be nicer to move the triggerAutoGC()
// logic up into the LocalStore base class.  However, for now it is more
// convenient to be able to use RocksDbLocalStore's ioPool_ to schedule the
// work.  We could use the EdenServer's main thread pool from the LocalStore
// code, but the gc operation can take a significant amount of time, and it
// seems unfortunate to tie up one of the main pool threads for potentially
// multiple minutes.
void RocksDbLocalStore::triggerAutoGC(SizeSummary before) {
  {
    auto state = autoGCState_.wlock();
    if (state->inProgress_) {
      XLOG(WARN) << "skipping local store garbage collection: "
                    "another GC job is still running";
      fb303::fbData->incrementCounter(
          folly::to<string>(statsPrefix_, "auto_gc.schedule_failure"));
      return;
    }
    fb303::fbData->setCounter(
        folly::to<string>(statsPrefix_, "auto_gc.running"), 1);

    fb303::fbData->incrementCounter(
        folly::to<string>(statsPrefix_, "auto_gc.schedule_count"));
    state->startTime_ = std::chrono::steady_clock::now();
    state->inProgress_ = true;
  }

  ioPool_.add([store = getSharedFromThis(), before] {
    try {
      for (auto& ks : KeySpace::kAll) {
        if (before.excessiveKeySpaces.test(ks->index)) {
          store->clearKeySpace(ks);
          store->compactKeySpace(ks);
        }
      }
    } catch (const std::exception& ex) {
      XLOG(ERR) << "error during automatic local store garbage collection: "
                << folly::exceptionStr(ex);
      store->autoGCFinished(/*successful=*/false, before.ephemeral);
      return;
    }
    store->autoGCFinished(/*successful=*/true, before.ephemeral);
  });
}

void RocksDbLocalStore::autoGCFinished(
    bool successful,
    uint64_t ephemeralSizeBefore) {
  auto ephemeralSizeAfter =
      computeStats(/*publish=*/false, /*config=*/nullptr).ephemeral;

  auto state = autoGCState_.wlock();
  state->inProgress_ = false;

  auto endTime = std::chrono::steady_clock::now();
  auto duration = endTime - state->startTime_;
  auto durationMS =
      std::chrono::duration_cast<std::chrono::milliseconds>(duration).count();

  // TODO: log the column names in the structured event
  structuredLogger_->logEvent(RocksDbAutomaticGc{
      duration_cast<std::chrono::duration<double>>(duration).count(),
      successful,
      static_cast<int64_t>(ephemeralSizeBefore),
      static_cast<int64_t>(ephemeralSizeAfter)});

  fb303::fbData->setCounter(
      folly::to<string>(statsPrefix_, "auto_gc.running"), 0);
  fb303::fbData->setCounter(
      folly::to<string>(statsPrefix_, "auto_gc.last_run"), time(nullptr));
  fb303::fbData->setCounter(
      folly::to<string>(statsPrefix_, "auto_gc.last_run_succeeded"),
      successful ? 1 : 0);
  fb303::fbData->setCounter(
      folly::to<string>(statsPrefix_, "auto_gc.last_duration_ms"), durationMS);

  if (successful) {
    fb303::fbData->incrementCounter(
        folly::to<string>(statsPrefix_, "auto_gc.success"));
  } else {
    fb303::fbData->incrementCounter(
        folly::to<string>(statsPrefix_, "auto_gc.failure"));
  }
}

void RocksDbLocalStore::throwStoreClosedError(
    folly::Synchronized<RocksDbLocalStore::RockDBState>::ConstRLockedPtr&
        lockedState) const {
  // It might be nicer to throw an EdenError exception here.
  // At the moment we don't simply due to library dependency ordering in the
  // CMake-based build.  We should ideally restructure the CMake-based build to
  // more closely match our Buck-based library configuration.
  throwf<std::runtime_error>(
      "the RocksDB local store is already closed. Localstore: {}, state: {}",
      fmt::ptr(this),
      fmt::ptr(&*lockedState));
}

void RocksDbLocalStore::throwStoreNotYetOpenedError(
    folly::Synchronized<RocksDbLocalStore::RockDBState>::ConstRLockedPtr&
        lockedState) const {
  // see comment about EdenError in throwStoreClosedError
  throwf<std::runtime_error>(
      "the RocksDB local store has not yet been opened. Localstore: {}, state: {}",
      fmt::ptr(this),
      fmt::ptr(&*lockedState));
}

} // namespace facebook::eden
