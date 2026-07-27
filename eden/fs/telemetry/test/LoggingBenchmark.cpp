/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <benchmark/benchmark.h>

#include <atomic>

#include <folly/init/Init.h>
#include <folly/json/json.h>

#include "eden/common/telemetry/DynamicEvent.h"
#include "eden/common/telemetry/SubprocessScribeLogger.h"
#include "eden/common/utils/RefPtr.h"
#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/telemetry/XplatKeys.h"
#include "eden/fs/telemetry/facebook/EdenTelemetryIdentity.h"
#include "eden/fs/telemetry/facebook/XplatLogger.h"
#include "eden/fs/telemetry/facebook/XplatTransforms.h"
#include "eden/fs/telemetry/facebook/if/gen-cpp2/edenfs_file_access_entry_types.h"
#include "scribe/api/producer/structured/CompactMessage.h"

using namespace facebook::eden;

// Representative data for both paths — kept identical for fair comparison.
constexpr std::string_view kRepo{"fbsource"};
constexpr std::string_view kDirectory{"fbcode/eden/fs/inodes"};
constexpr std::string_view kFilename{"TreeInode.cpp"};
constexpr std::string_view kSource{"FUSE_READ"};
constexpr std::string_view kSourceDetail{"read(fd=42, offset=0, len=4096)"};

namespace {

// Populate a DynamicEvent the same way the legacy scribe_cat file-access path
// did, for an apples-to-apples comparison against the XplatLogger path.
DynamicEvent makeScribeCatEvent() {
  DynamicEvent event;
  event.addString("repo", std::string{kRepo});
  event.addString("directory", std::string{kDirectory});
  event.addString("filename", std::string{kFilename});
  event.addString("source", std::string{kSource});
  event.addString("source_detail", std::string{kSourceDetail});
  return event;
}

// Build the Scuba JSON document from a DynamicEvent, matching
// ScubaStructuredLogger::logDynamicEvent().
std::string serializeScubaDynamicEvent(const DynamicEvent& event) {
  folly::dynamic document = folly::dynamic::object;

  const auto& intMap = event.getIntMap();
  if (!intMap.empty()) {
    folly::dynamic o = folly::dynamic::object;
    for (const auto& [key, value] : intMap) {
      o[key] = value;
    }
    document["int"] = std::move(o);
  }

  const auto& stringMap = event.getStringMap();
  if (!stringMap.empty()) {
    folly::dynamic o = folly::dynamic::object;
    for (const auto& [key, value] : stringMap) {
      o[key] = value;
    }
    document["normal"] = std::move(o);
  }

  const auto& doubleMap = event.getDoubleMap();
  if (!doubleMap.empty()) {
    folly::dynamic o = folly::dynamic::object;
    for (const auto& [key, value] : doubleMap) {
      o[key] = value;
    }
    document["double"] = std::move(o);
  }

  return folly::toJson(document);
}

// Build an EdenTelemetryIdentity with representative session data.
EdenTelemetryIdentity makeIdentity() {
  EdenTelemetryIdentity identity;
  identity.sessionId = 12345;
  identity.username = "testuser";
  identity.hostname = "devvm001.prn1.facebook.com";
  identity.os = "Linux";
  identity.osVersion = "6.16.1";
  identity.appVersion = "20260414-123456";
  return identity;
}

// Populate a Thrift EdenfsFileAccessEntry, mirroring toFileAccessEntry()
// in XplatLogger.cpp.
telemetry::EdenfsFileAccessEntry makeThriftEntry(
    const EdenTelemetryIdentity& identity) {
  telemetry::EdenfsFileAccessEntry entry;
  entry.username() = identity.username;
  entry.hostname() = identity.hostname;
  entry.os() = identity.os;
  entry.osver() = identity.osVersion;
  entry.edenver() = identity.appVersion;
  entry.logged_by() = "edenfs";
  entry.repo() = std::string{kRepo};
  entry.directory() = std::string{kDirectory};
  entry.filename() = std::string{kFilename};
  entry.source() = std::string{kSource};
  entry.source_detail() = std::string{kSourceDetail};
  entry.session_id() = static_cast<int64_t>(identity.sessionId);
  return entry;
}

} // namespace

// ---------------------------------------------------------------------------
// Level 1: Serialization only
// ---------------------------------------------------------------------------

// Measures: DynamicEvent population + folly::toJson (JSON serialization).
// This is the CPU cost of the scribe_cat serialization path.
static void BM_ScribeCat_Serialize(benchmark::State& state) {
  for (auto _ : state) {
    auto event = makeScribeCatEvent();
    auto json = serializeScubaDynamicEvent(event);
    benchmark::DoNotOptimize(json.data());
    benchmark::ClobberMemory();
  }
}
BENCHMARK(BM_ScribeCat_Serialize);

// Measures: Thrift struct population + Compact Protocol serialization.
// This is the CPU cost of the XplatLogger serialization path.
static void BM_XplatLogger_Serialize(benchmark::State& state) {
  const auto identity = makeIdentity();
  for (auto _ : state) {
    auto entry = makeThriftEntry(identity);
    auto compact = scribe::api::structured::makeCompact(entry);
    benchmark::DoNotOptimize(compact);
    benchmark::ClobberMemory();
  }
}
BENCHMARK(BM_XplatLogger_Serialize);

// ---------------------------------------------------------------------------
// Level 2: Enqueue latency (what the caller actually pays)
// ---------------------------------------------------------------------------

// Measures: JSON serialization + SubprocessScribeLogger::log() (mutex +
// queue push). Uses /bin/cat as a sink that reads and discards messages.
static void BM_ScribeCat_Enqueue(benchmark::State& state) {
  static std::atomic<SubprocessScribeLogger*> sharedLogger{nullptr};
  static std::atomic<int> teardownCount{0};
  std::unique_ptr<SubprocessScribeLogger> ownedLogger;

  if (state.thread_index() == 0) {
    teardownCount.store(0, std::memory_order_relaxed);
    ownedLogger = std::make_unique<SubprocessScribeLogger>(
        std::vector<std::string>{"/bin/cat"});
    sharedLogger.store(ownedLogger.get(), std::memory_order_release);
  }

  while (sharedLogger.load(std::memory_order_acquire) == nullptr) {
  }

  for (auto _ : state) {
    auto* logger = sharedLogger.load(std::memory_order_acquire);
    auto event = makeScribeCatEvent();
    auto json = serializeScubaDynamicEvent(event);
    logger->log(std::move(json));
  }

  teardownCount.fetch_add(1, std::memory_order_acq_rel);

  if (state.thread_index() == 0) {
    while (teardownCount.load(std::memory_order_acquire) != state.threads()) {
    }
    ownedLogger.reset();
    sharedLogger.store(nullptr, std::memory_order_release);
  }
}
BENCHMARK(BM_ScribeCat_Enqueue);

// Measures: Thrift serialization + enqueueMessage() (AsyncScope coroutine
// dispatch). Fire-and-forget — ScribeD connection failures are silently
// dropped, so this works with or without a local ScribeD.
static void BM_XplatLogger_Enqueue(benchmark::State& state) {
  static std::atomic<XplatLogger*> sharedLogger{nullptr};
  static std::atomic<int> teardownCount{0};
  std::unique_ptr<XplatLogger> ownedLogger;

  if (state.thread_index() == 0) {
    teardownCount.store(0, std::memory_order_relaxed);
    ownedLogger = std::make_unique<XplatLogger>(
        makeIdentity(), makeRefPtr<EdenStats>(), nullptr);
    ownedLogger->registerTransform(
        "perfpipe_edenfs_file_accesses",
        "GeneratedEdenfsFileAccessesLoggerConfig",
        fileAccessTransform);
    sharedLogger.store(ownedLogger.get(), std::memory_order_release);
  }

  while (sharedLogger.load(std::memory_order_acquire) == nullptr) {
  }

  for (auto _ : state) {
    auto* logger = sharedLogger.load(std::memory_order_acquire);
    DynamicEvent event;
    event.addString(std::string{xplat_keys::kRepo}, std::string{kRepo});
    event.addString(
        std::string{xplat_keys::kDirectory}, std::string{kDirectory});
    event.addString(std::string{xplat_keys::kFilename}, std::string{kFilename});
    event.addString(std::string{xplat_keys::kSource}, std::string{kSource});
    event.addString(
        std::string{xplat_keys::kSourceDetail}, std::string{kSourceDetail});
    logger->logEvent("perfpipe_edenfs_file_accesses", event);
  }

  teardownCount.fetch_add(1, std::memory_order_acq_rel);

  if (state.thread_index() == 0) {
    while (teardownCount.load(std::memory_order_acquire) != state.threads()) {
    }
    ownedLogger.reset();
    sharedLogger.store(nullptr, std::memory_order_release);
  }
}
BENCHMARK(BM_XplatLogger_Enqueue);

// ---------------------------------------------------------------------------
// Level 3: Throughput (burst of N events)
// ---------------------------------------------------------------------------

static void BM_ScribeCat_Throughput(benchmark::State& state) {
  const auto burstSize = state.range(0);
  auto logger = std::make_unique<SubprocessScribeLogger>(
      std::vector<std::string>{"/bin/cat"});

  for (auto _ : state) {
    for (int64_t i = 0; i < burstSize; ++i) {
      auto event = makeScribeCatEvent();
      auto json = serializeScubaDynamicEvent(event);
      logger->log(std::move(json));
    }
  }
  state.SetItemsProcessed(state.iterations() * static_cast<int64_t>(burstSize));
}
BENCHMARK(BM_ScribeCat_Throughput)->Arg(100)->Arg(1000)->Arg(10000);

static void BM_XplatLogger_Throughput(benchmark::State& state) {
  const auto burstSize = state.range(0);
  auto logger = std::make_unique<XplatLogger>(
      makeIdentity(), makeRefPtr<EdenStats>(), nullptr);
  logger->registerTransform(
      "perfpipe_edenfs_file_accesses",
      "GeneratedEdenfsFileAccessesLoggerConfig",
      fileAccessTransform);

  for (auto _ : state) {
    for (int64_t i = 0; i < burstSize; ++i) {
      DynamicEvent event;
      event.addString(std::string{xplat_keys::kRepo}, std::string{kRepo});
      event.addString(
          std::string{xplat_keys::kDirectory}, std::string{kDirectory});
      event.addString(
          std::string{xplat_keys::kFilename}, std::string{kFilename});
      event.addString(std::string{xplat_keys::kSource}, std::string{kSource});
      event.addString(
          std::string{xplat_keys::kSourceDetail}, std::string{kSourceDetail});
      logger->logEvent("perfpipe_edenfs_file_accesses", event);
    }
  }
  state.SetItemsProcessed(state.iterations() * static_cast<int64_t>(burstSize));
}
BENCHMARK(BM_XplatLogger_Throughput)->Arg(100)->Arg(1000)->Arg(10000);

// ---------------------------------------------------------------------------
// Level 4: Multi-thread enqueue
// ---------------------------------------------------------------------------

BENCHMARK(BM_ScribeCat_Enqueue)
    ->Threads(2)
    ->Name("BM_ScribeCat_Enqueue/threads:2");
BENCHMARK(BM_ScribeCat_Enqueue)
    ->Threads(4)
    ->Name("BM_ScribeCat_Enqueue/threads:4");
BENCHMARK(BM_ScribeCat_Enqueue)
    ->Threads(8)
    ->Name("BM_ScribeCat_Enqueue/threads:8");

BENCHMARK(BM_XplatLogger_Enqueue)
    ->Threads(2)
    ->Name("BM_XplatLogger_Enqueue/threads:2");
BENCHMARK(BM_XplatLogger_Enqueue)
    ->Threads(4)
    ->Name("BM_XplatLogger_Enqueue/threads:4");
BENCHMARK(BM_XplatLogger_Enqueue)
    ->Threads(8)
    ->Name("BM_XplatLogger_Enqueue/threads:8");

// ---------------------------------------------------------------------------
// Level 5: Parameterized config benchmarks
// ---------------------------------------------------------------------------

namespace {

std::shared_ptr<ReloadableConfig> makeConfigWithValues(
    size_t queueLimitBytes,
    size_t maxBatchSize) {
  auto config = EdenConfig::createTestEdenConfig();
  config->xplatLoggerQueueLimitBytes.setValue(
      queueLimitBytes, ConfigSourceType::CommandLine);
  config->xplatLoggerMaxBatchSize.setValue(
      maxBatchSize, ConfigSourceType::CommandLine);
  return std::make_shared<ReloadableConfig>(std::move(config));
}

void logBurst(XplatLogger& logger, int64_t burstSize) {
  for (int64_t i = 0; i < burstSize; ++i) {
    DynamicEvent event;
    event.addString(std::string{xplat_keys::kRepo}, std::string{kRepo});
    event.addString(
        std::string{xplat_keys::kDirectory}, std::string{kDirectory});
    event.addString(std::string{xplat_keys::kFilename}, std::string{kFilename});
    event.addString(std::string{xplat_keys::kSource}, std::string{kSource});
    event.addString(
        std::string{xplat_keys::kSourceDetail}, std::string{kSourceDetail});
    logger.logEvent("perfpipe_edenfs_file_accesses", event);
  }
}

} // namespace

// Queue limit bytes: burst throughput with varying queue sizes.
// Tests how queue size affects drop rate under burst load.
static void BM_XplatLogger_QueueLimit(benchmark::State& state) {
  const auto queueLimitBytes = static_cast<size_t>(state.range(0));
  constexpr int64_t kBurstSize = 10000;
  auto reloadableConfig =
      makeConfigWithValues(queueLimitBytes, 100 /* default batch size */);
  auto logger = std::make_unique<XplatLogger>(
      makeIdentity(), makeRefPtr<EdenStats>(), reloadableConfig);
  logger->registerTransform(
      "perfpipe_edenfs_file_accesses",
      "GeneratedEdenfsFileAccessesLoggerConfig",
      fileAccessTransform);

  for (auto _ : state) {
    logBurst(*logger, kBurstSize);
  }
  state.SetItemsProcessed(state.iterations() * kBurstSize);
}
BENCHMARK(BM_XplatLogger_QueueLimit)
    ->Arg(65536)
    ->Arg(131072)
    ->Arg(262144)
    ->Arg(524288)
    ->Arg(1048576);

// Max batch size: throughput with varying batch sizes.
// Tests how batch size affects throughput and RPC overhead.
static void BM_XplatLogger_BatchSize(benchmark::State& state) {
  const auto maxBatchSize = static_cast<size_t>(state.range(0));
  constexpr int64_t kBurstSize = 10000;
  auto reloadableConfig =
      makeConfigWithValues(131072 /* default queue limit */, maxBatchSize);
  auto logger = std::make_unique<XplatLogger>(
      makeIdentity(), makeRefPtr<EdenStats>(), reloadableConfig);
  logger->registerTransform(
      "perfpipe_edenfs_file_accesses",
      "GeneratedEdenfsFileAccessesLoggerConfig",
      fileAccessTransform);

  for (auto _ : state) {
    logBurst(*logger, kBurstSize);
  }
  state.SetItemsProcessed(state.iterations() * kBurstSize);
}
BENCHMARK(BM_XplatLogger_BatchSize)
    ->Arg(10)
    ->Arg(50)
    ->Arg(100)
    ->Arg(200)
    ->Arg(500);

// Combined: queue limit x batch size matrix.
// Tests interesting combinations of queue limit and batch size.
static void BM_XplatLogger_QueueBatchCombo(benchmark::State& state) {
  const auto queueLimitBytes = static_cast<size_t>(state.range(0));
  const auto maxBatchSize = static_cast<size_t>(state.range(1));
  constexpr int64_t kBurstSize = 10000;
  auto reloadableConfig = makeConfigWithValues(queueLimitBytes, maxBatchSize);
  auto logger = std::make_unique<XplatLogger>(
      makeIdentity(), makeRefPtr<EdenStats>(), reloadableConfig);
  logger->registerTransform(
      "perfpipe_edenfs_file_accesses",
      "GeneratedEdenfsFileAccessesLoggerConfig",
      fileAccessTransform);

  for (auto _ : state) {
    logBurst(*logger, kBurstSize);
  }
  state.SetItemsProcessed(state.iterations() * kBurstSize);
}
BENCHMARK(BM_XplatLogger_QueueBatchCombo)
    ->Args({32768, 10})
    ->Args({32768, 25})
    ->Args({32768, 50})
    ->Args({65536, 10})
    ->Args({65536, 25})
    ->Args({65536, 50})
    ->Args({65536, 100})
    ->Args({65536, 200})
    ->Args({131072, 10})
    ->Args({131072, 50})
    ->Args({131072, 100})
    ->Args({131072, 200})
    ->Args({262144, 50})
    ->Args({262144, 100})
    ->Args({262144, 200})
    ->Args({524288, 500})
    ->Args({1048576, 100});

// ---------------------------------------------------------------------------

int main(int argc, char** argv) {
  folly::Init init(&argc, &argv);
  benchmark::Initialize(&argc, argv);
  benchmark::RunSpecifiedBenchmarks();
  benchmark::Shutdown();
  return 0;
}
