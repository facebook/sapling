/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <sysexits.h>
#include <memory>
#include <optional>

#include <folly/Range.h>
#include <folly/String.h>
#include <folly/container/Array.h>
#include <folly/container/Enumerate.h>
#include <folly/init/Init.h>
#include <folly/logging/Init.h>
#include <folly/logging/xlog.h>
#include <folly/stop_watch.h>

#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/fuse/privhelper/UserInfo.h"
#include "eden/fs/service/EdenInit.h"
#include "eden/fs/service/EdenStateDir.h"
#include "eden/fs/store/KeySpaces.h"
#include "eden/fs/store/RocksDbLocalStore.h"
#include "eden/fs/utils/FaultInjector.h"

using namespace facebook::eden;
using folly::make_array;
using folly::StringPiece;
using std::make_unique;
using std::optional;

FOLLY_INIT_LOGGING_CONFIG("eden=DBG2; default:async=true");

DEFINE_string(keySpace, "", "operate on just a single key space");

namespace {

LocalStore::KeySpace stringToKeySpace(StringPiece name) {
  if (name == "blob") {
    return LocalStore::BlobFamily;
  } else if (name == "blobmeta") {
    return LocalStore::BlobMetaDataFamily;
  } else if (name == "tree") {
    return LocalStore::TreeFamily;
  } else if (name == "hgproxyhash") {
    return LocalStore::HgProxyHashFamily;
  } else if (name == "hgcommit2tree") {
    return LocalStore::HgCommitToTreeFamily;
  }
  throw ArgumentError("unknown key space \"", name, "\"");
}

optional<LocalStore::KeySpace> getKeySpace() {
  if (FLAGS_keySpace.empty()) {
    return std::nullopt;
  }
  return stringToKeySpace(FLAGS_keySpace);
}

class Command {
 public:
  Command()
      : userInfo_(UserInfo::lookup()),
        config_(getEdenConfig(userInfo_)),
        edenDir_([this]() {
          XLOG(INFO) << "Using Eden directory: " << config_->getEdenDir();
          return EdenStateDir(config_->getEdenDir());
        }()) {
    if (!edenDir_.acquireLock()) {
      throw ArgumentError(
          "error: failed to acquire the Eden lock\n"
          "This utility cannot be used while edenfs is running.");
    }
  }
  virtual ~Command() {}

  virtual void run() = 0;

 protected:
  AbsolutePath getLocalStorePath() const {
    return edenDir_.getPath() + "storage/rocks-db"_relpath;
  }

  std::unique_ptr<RocksDbLocalStore> openLocalStore(RocksDBOpenMode mode) {
    folly::stop_watch<std::chrono::milliseconds> watch;
    const auto rocksPath = getLocalStorePath();
    ensureDirectoryExists(rocksPath);
    auto localStore =
        make_unique<RocksDbLocalStore>(rocksPath, &faultInjector_, mode);
    XLOG(INFO) << "Opened RocksDB store in "
               << (mode == RocksDBOpenMode::ReadOnly ? "read-only"
                                                     : "read-write")
               << " mode in " << (watch.elapsed().count() / 1000.0)
               << " seconds.";
    return localStore;
  }

  UserInfo userInfo_;
  std::shared_ptr<EdenConfig> config_;
  EdenStateDir edenDir_;
  FaultInjector faultInjector_{/*enabled=*/false};
};

class CommandFactory {
 public:
  virtual ~CommandFactory() {}
  virtual StringPiece name() const = 0;
  virtual StringPiece help() const = 0;
  virtual std::unique_ptr<Command> create() = 0;
};

template <typename CommandType>
class CommandFactoryT : public CommandFactory {
  StringPiece name() const override {
    return CommandType::name;
  }
  StringPiece help() const override {
    return CommandType::help;
  }
  std::unique_ptr<Command> create() override {
    return make_unique<CommandType>();
  }
};

class GcCommand : public Command {
 public:
  static constexpr auto name = StringPiece("gc");
  static constexpr auto help =
      StringPiece("Clear cached data then compact storage");

  void run() override {
    auto keySpace = getKeySpace();
    auto localStore = openLocalStore(RocksDBOpenMode::ReadWrite);
    if (keySpace) {
      localStore->clearKeySpace(*keySpace);
      localStore->compactKeySpace(*keySpace);
    } else {
      localStore->clearCachesAndCompactAll();
    }
  }
};

class ClearCommand : public Command {
 public:
  static constexpr auto name = StringPiece("clear");
  static constexpr auto help =
      StringPiece("Clear cached data without compacting storage");

  void run() override {
    auto keySpace = getKeySpace();
    auto localStore = openLocalStore(RocksDBOpenMode::ReadWrite);
    if (keySpace) {
      localStore->clearKeySpace(*keySpace);
    } else {
      localStore->clearCaches();
    }
  }
};

class CompactCommand : public Command {
 public:
  static constexpr auto name = StringPiece("compact");
  static constexpr auto help = StringPiece("Compact the RocksDB storage");

  void run() override {
    auto keySpace = getKeySpace();
    auto localStore = openLocalStore(RocksDBOpenMode::ReadWrite);
    if (keySpace) {
      localStore->compactKeySpace(*keySpace);
    } else {
      localStore->compactStorage();
    }
  }
};

class RepairCommand : public Command {
 public:
  static constexpr auto name = StringPiece("repair");
  static constexpr auto help = StringPiece(
      "Force a repair of the RocksDB storage, even if it does not look corrupt");

  void run() override {
    RocksDbLocalStore::repairDB(getLocalStorePath());
  }
};

class ShowSizesCommand : public Command {
 public:
  static constexpr auto name = StringPiece("show_sizes");
  static constexpr auto help =
      StringPiece("Report approximate sizes of each key space.");

  void run() override {
    auto localStore = openLocalStore(RocksDBOpenMode::ReadOnly);

    for (const auto& iter : folly::enumerate(kKeySpaceRecords)) {
      LOG(INFO) << "Column family \"" << iter->name << "\": "
                << folly::prettyPrint(
                       localStore->getApproximateSize(
                           static_cast<LocalStore::KeySpace>(iter.index)),
                       folly::PRETTY_BYTES_METRIC);
    }
  }
};

std::unique_ptr<Command> createCommand(StringPiece name) {
  auto commands = make_array<std::unique_ptr<CommandFactory>>(
      make_unique<CommandFactoryT<GcCommand>>(),
      make_unique<CommandFactoryT<ClearCommand>>(),
      make_unique<CommandFactoryT<CompactCommand>>(),
      make_unique<CommandFactoryT<RepairCommand>>(),
      make_unique<CommandFactoryT<ShowSizesCommand>>());

  std::unique_ptr<Command> command;
  for (const auto& factory : commands) {
    if (factory->name() == name) {
      return factory->create();
    }
  }
  throw ArgumentError("unknown command \"", name, "\"");
}

} // namespace

int main(int argc, char** argv) {
  folly::init(&argc, &argv);
  if (argc != 2) {
    fprintf(stderr, "error: no command specified\n");
    fprintf(stderr, "usage: eden_store_util COMMAND\n");
    return EX_SOFTWARE;
  }

  std::unique_ptr<Command> command;
  try {
    command = createCommand(argv[1]);
  } catch (const ArgumentError& ex) {
    fprintf(stderr, "error: %s\n", ex.what());
    return EX_SOFTWARE;
  }

  command->run();
  return 0;
}
