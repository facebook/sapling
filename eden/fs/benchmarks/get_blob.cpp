/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/io/async/EventBaseThread.h>
#include <thrift/lib/cpp2/async/HeaderClientChannel.h>
#include <filesystem>
#include <fstream>
#include <sstream>
#include "eden/common/utils/PathFuncs.h"
#include "eden/common/utils/SpawnedProcess.h"
#include "eden/common/utils/benchharness/Bench.h"
#include "eden/fs/service/ThriftGetObjectImpl.h"
#include "eden/fs/service/gen-cpp2/EdenService.h"

DEFINE_string(repo, "", "Repository to run the benchmark against");
DEFINE_string(
    blob_ids,
    "",
    "Comma-separated list of blob IDs to fetch (hex format)");
DEFINE_string(
    path,
    "fbcode/eden/scm/tests",
    "Path/glob pattern to get blob IDs from (default: fbcode/eden/scm/tests)");
DEFINE_bool(
    local_store_only,
    false,
    "Only fetch from local store (no network)");
DEFINE_bool(memory_cache_only, false, "Only fetch from memory cache");
DEFINE_bool(disk_cache_only, false, "Only fetch from disk cache");
DEFINE_bool(remote_only, false, "Only fetch from remote backing store");

namespace {

using namespace facebook::eden;

#ifdef _WIN32
std::optional<AbsolutePath> getSocketPathFromConfig(
    const AbsolutePath& mountPath) {
  auto configPath = mountPath + ".eden/config"_relpath;

  if (!std::filesystem::exists(configPath.asString())) {
    return std::nullopt;
  }

  std::ifstream configFile(configPath.asString());
  if (!configFile.is_open()) {
    return std::nullopt;
  }

  std::string line;
  while (std::getline(configFile, line)) {
    line.erase(0, line.find_first_not_of(" \t\r\n"));
    line.erase(line.find_last_not_of(" \t\r\n") + 1);

    if (line.find("socket = ") == 0) {
      std::string socketPart = line.substr(9); // Remove "socket = "

      if (!socketPart.empty() &&
          ((socketPart.front() == '"' && socketPart.back() == '"') ||
           (socketPart.front() == '\'' && socketPart.back() == '\''))) {
        socketPart = socketPart.substr(1, socketPart.length() - 2);
      }

      try {
        return canonicalPath(socketPart);
      } catch (const std::exception&) {
        return std::nullopt;
      }
    }
  }

  return std::nullopt;
}
#endif

AbsolutePath getEdenSocketPath(const AbsolutePath& mountPath) {
#ifdef _WIN32
  // On Windows, always read from .eden/config
  auto socketPath = getSocketPathFromConfig(mountPath);
  if (socketPath) {
    return *socketPath;
  }
  throw std::runtime_error(
      "Could not find socket path in .eden/config file for Windows mount: " +
      mountPath.asString());
#else
  // On Linux and Mac, we can assume the default socket path
  return mountPath + ".eden/socket"_relpath;
#endif
}

std::vector<std::string> getBlobIdsFromPath(
    const AbsolutePath& repoPath,
    const std::string& pathPattern) {
  std::vector<std::string> blobIds;

  try {
    // First, ensure files are loaded by running eden glob
    {
      SpawnedProcess::Options globOpts;
      globOpts.pipeStdout();
      globOpts.pipeStderr();
      globOpts.chdir(repoPath);

      std::string globPattern = pathPattern + "/*";
      SpawnedProcess globProc(
          {"eden", "glob", globPattern}, std::move(globOpts));

      auto [globStdout, globStderr] = globProc.communicate();
      globProc.waitChecked();
    }

    // Now get the object IDs using debug inode
    SpawnedProcess::Options opts;
    opts.pipeStdout();
    opts.pipeStderr();

    std::string targetPath = repoPath.stringWithoutUNC() + "/" + pathPattern;

    SpawnedProcess proc(
        {"eden", "debug", "inode", targetPath}, std::move(opts));

    auto [stdoutOutput, stderrOutput] = proc.communicate();
    proc.waitChecked();

    // Parse the output line by line and extract blob IDs
    std::istringstream iss(stdoutOutput);
    std::string line;
    while (std::getline(iss, line)) {
      // Trim whitespace
      line.erase(0, line.find_first_not_of(" \t\r\n"));
      line.erase(line.find_last_not_of(" \t\r\n") + 1);

      if (line.empty()) {
        continue;
      }

      // Look for lines containing a colon (indicating a hash:path pattern)
      size_t colonPos = line.find(':');
      if (colonPos != std::string::npos && colonPos >= 40) {
        // Extract potential 40-character hash before the colon
        std::string candidate = line.substr(colonPos - 40, 40);

        // Validate it's a valid hex string
        bool isValid = true;
        for (char c : candidate) {
          if (!std::isxdigit(c)) {
            isValid = false;
            break;
          }
        }

        if (isValid && candidate.length() == 40) {
          blobIds.push_back(candidate);
        }
      }
    }

    if (blobIds.empty()) {
      throw std::runtime_error(
          "No object IDs found for path pattern: " + pathPattern);
    }

    return blobIds;
  } catch (const std::exception& e) {
    throw std::runtime_error(
        std::string("Failed to execute command to get blob IDs from path '") +
        pathPattern + "': " + e.what());
  }
}

std::vector<std::string> parseBlobIds(const AbsolutePath& repoPath) {
  std::vector<std::string> blobIds;

  if (!FLAGS_blob_ids.empty()) {
    // Parse comma-separated blob IDs
    std::string ids = FLAGS_blob_ids;
    size_t start = 0;
    size_t end = 0;

    while ((end = ids.find(',', start)) != std::string::npos) {
      std::string id = ids.substr(start, end - start);
      // Trim whitespace
      id.erase(0, id.find_first_not_of(" \t\r\n"));
      id.erase(id.find_last_not_of(" \t\r\n") + 1);
      if (!id.empty()) {
        blobIds.push_back(id);
      }
      start = end + 1;
    }

    // Handle the last ID
    std::string id = ids.substr(start);
    id.erase(0, id.find_first_not_of(" \t\r\n"));
    id.erase(id.find_last_not_of(" \t\r\n") + 1);
    if (!id.empty()) {
      blobIds.push_back(id);
    }
  } else {
    blobIds = getBlobIdsFromPath(repoPath, FLAGS_path);
  }

  if (blobIds.empty()) {
    throw std::invalid_argument(
        "No blob IDs provided. Use --blob_ids, --path, or let it use the default path");
  }

  return blobIds;
}

DataFetchOriginSet getOriginFlags() {
  DataFetchOriginFlags origins{};

  if (FLAGS_local_store_only) {
    origins = FROMWHERE_LOCAL_BACKING_STORE;
  } else if (FLAGS_memory_cache_only) {
    origins = FROMWHERE_MEMORY_CACHE;
  } else if (FLAGS_disk_cache_only) {
    origins = FROMWHERE_DISK_CACHE;
  } else if (FLAGS_remote_only) {
    origins = FROMWHERE_REMOTE_BACKING_STORE;
  } else {
    // Default: try all origins
    origins = FROMWHERE_LOCAL_BACKING_STORE | FROMWHERE_MEMORY_CACHE |
        FROMWHERE_DISK_CACHE | FROMWHERE_REMOTE_BACKING_STORE;
  }

  return origins.asRaw();
}

AbsolutePath validateArguments() {
  if (FLAGS_repo.empty()) {
    throw std::invalid_argument("A repo must be passed in");
  }

  return canonicalPath(FLAGS_repo);
}

void eden_debug_get_blob(benchmark::State& state) {
  auto path = validateArguments();
  auto blobIds = parseBlobIds(path);
  if (blobIds.empty()) {
    throw std::invalid_argument("No blob IDs provided");
  }
  auto origins = getOriginFlags();

  // Use the new socket identification logic
  auto socketPath = getEdenSocketPath(path);

  // Create eventbase and client for this thread
  auto evbThread = folly::EventBaseThread();
  auto eventBase = evbThread.getEventBase();

  auto socket = folly::AsyncSocket::newSocket(
      eventBase, folly::SocketAddress::makeFromPath(socketPath.view()));
  auto channel =
      apache::thrift::HeaderClientChannel::newChannel(std::move(socket));
  auto client = std::make_unique<EdenServiceAsyncClient>(std::move(channel));

  // Pre-create requests for all blob IDs to avoid overhead during benchmark
  std::vector<DebugGetScmBlobRequest> requests;
  requests.reserve(blobIds.size());

  for (const auto& blobId : blobIds) {
    DebugGetScmBlobRequest request;
    request.mountId() = MountId();
    request.mountId()->mountPoint() = path.view();
    request.id() = blobId;
    request.origins() = origins;
    requests.push_back(std::move(request));
  }

  size_t blobIndex = 0;
  size_t totalErrors = 0;

  for (auto _ : state) {
    auto totalTime = std::chrono::duration<double>::zero();
    for (auto& request : requests) {
      blobIndex++;

      auto start = std::chrono::high_resolution_clock::now();
      auto result =
          client->semifuture_debugGetBlob(request).via(eventBase).get();
      auto end = std::chrono::high_resolution_clock::now();

      benchmark::DoNotOptimize(result);

      // Check for errors
      bool hasError = true;
      if (apache::thrift::is_non_optional_field_set_manually_or_by_serializer(
              result.blobs()) &&
          !result.blobs()->empty()) {
        for (const auto& blobWithOrigin : *result.blobs()) {
          if (apache::thrift::
                  is_non_optional_field_set_manually_or_by_serializer(
                      blobWithOrigin.blob())) {
            const auto& blobOrError = *blobWithOrigin.blob();
            if (blobOrError.getType() == ScmBlobOrError::Type::blob) {
              const auto& blobData = blobOrError.get_blob();
              if (!blobData.empty()) {
                hasError = false;
              }
            }
          }
        }
      }

      if (hasError) {
        totalErrors++;
      }

      auto elapsed = std::chrono::duration_cast<std::chrono::duration<double>>(
          end - start);
      totalTime += elapsed;
    }
    state.SetIterationTime(totalTime.count());
  }

  // Report counters
  state.counters["total_requests"] = blobIndex;
  state.counters["total_errors"] = totalErrors;
}

BENCHMARK(eden_debug_get_blob)
    ->UseManualTime()
    ->Unit(benchmark::kMillisecond)
    ->Repetitions(10);

} // namespace

EDEN_BENCHMARK_MAIN();
