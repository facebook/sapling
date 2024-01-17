/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Range.h>
#include <folly/portability/GFlags.h>
#include <folly/portability/IOVec.h>
#include <optional>

#include "eden/fs/eden-config.h"
#include "eden/fs/model/BlobFwd.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/telemetry/LogEvent.h"
#include "eden/fs/utils/PathFuncs.h"
#include "eden/fs/utils/SpawnedProcess.h"

DECLARE_string(hgPath);
DECLARE_string(hgPythonPath);

namespace folly {
class IOBuf;
} // namespace folly

namespace facebook::eden {

class Hash20;
class HgManifestImporter;
class StoreResult;
class StructuredLogger;
class HgProxyHash;

/**
 * Options for this HgImporter.
 *
 * This is parsed from the initial CMD_STARTED response from `hg
 * debugedenimporthelper` , and contains details about the configuration
 * for this mercurial repository.
 */
struct ImporterOptions {
  /**
   * The paths to the treemanifest pack directories.
   * If this vector is empty treemanifest import should not be used.
   */
  std::vector<std::string> treeManifestPackPaths;

  /**
   * The name of the repo
   */
  std::string repoName;
};

class Importer {
 public:
  virtual ~Importer() = default;
};

/**
 * HgImporter provides an API for extracting data out of a mercurial
 * repository.
 *
 * Mercurial itself is in python, so some of the import logic runs as python
 * code.  HgImporter hides all of the interaction with the underlying python
 * code.
 *
 * HgImporter is thread-bound; use HgImporter only on the thread it was created
 * on.  To achieve parallelism multiple HgImporter objects can be created for
 * the same repository and used simultaneously.  HgImporter is thread-bound for
 * the following reasons:
 *
 * * HgImporter does not synchronize its own members.
 * * HgImporter accesses HgImporterThreadStats, and HgImporterThreadStats is
 * thread-bound.
 */
class HgImporter : public Importer {
 public:
  /**
   * Create a new HgImporter object that will import data from the specified
   * repository.
   */
  HgImporter(
      AbsolutePathPiece repoPath,
      EdenStatsPtr,
      std::optional<AbsolutePath> importHelperScript = std::nullopt);

  virtual ~HgImporter();

  ProcessStatus debugStopHelperProcess();

  const ImporterOptions& getOptions() const;

 private:
  /**
   * Chunk header flags.
   *
   * These are flag values, designed to be bitwise ORed with each other.
   */
  enum : uint32_t {
    FLAG_ERROR = 0x01,
    FLAG_MORE_CHUNKS = 0x02,
  };
  /**
   * hg debugedenimporthelper protocol version number.
   *
   * Bump this whenever you add new commands or change the command parameters
   * or response data.  This helps us identify if edenfs somehow ends up
   * using an incompatible version of hg debugedenimporthelper.
   *
   * This must be kept in sync with the PROTOCOL_VERSION field in
   * hg debugedenimporthelper.
   */
  enum : uint32_t {
    PROTOCOL_VERSION = 1,
  };
  /**
   * Flags for the CMD_STARTED response
   */
  enum StartFlag : uint32_t {
    TREEMANIFEST_SUPPORTED = 0x01,
    MONONOKE_SUPPORTED = 0x02,
    CAT_TREE_SUPPORTED = 0x04,
  };
  /**
   * Command type values.
   *
   * See hg debugedenimporthelper for a more complete description of the
   * request/response formats.
   */
  enum CommandType : uint32_t {
    CMD_STARTED = 0,
    CMD_RESPONSE = 1,
    CMD_MANIFEST = 2, // REMOVED
    CMD_OLD_CAT_FILE = 3,
    CMD_MANIFEST_NODE_FOR_COMMIT = 4, // REMOVED
    CMD_FETCH_TREE = 5,
    CMD_PREFETCH_FILES = 6, // REMOVED
    CMD_CAT_FILE = 7,
    CMD_GET_FILE_SIZE = 8,
    CMD_CAT_TREE = 9,
  };
  using TransactionID = uint32_t;
  struct ChunkHeader {
    TransactionID requestID;
    uint32_t command;
    uint32_t flags;
    uint32_t dataLength;
  };

  // Forbidden copy constructor and assignment operator
  HgImporter(const HgImporter&) = delete;
  HgImporter& operator=(const HgImporter&) = delete;

  void stopHelperProcess();
  /**
   * Wait for the helper process to send a CMD_STARTED response to indicate
   * that it has started successfully.  Process the response and finish
   * setting up member variables based on the data included in the response.
   */
  ImporterOptions waitForHelperStart();

  /**
   * Read a response chunk header from the helper process
   *
   * If the header indicates an error, this will read the full error message
   * and throw a std::runtime_error.
   *
   * This will throw an HgImporterError if there is an error communicating with
   * hg debugedenimporthelper subprocess (for instance, if the helper process
   * has exited, or if the response does not contain the expected transaction
   * ID).
   */
  ChunkHeader readChunkHeader(TransactionID txnID, folly::StringPiece cmdName);

  /**
   * Read the body of an error message, and throw it as an exception.
   */
  [[noreturn]] void readErrorAndThrow(const ChunkHeader& header);

  void readFromHelper(void* buf, uint32_t size, folly::StringPiece context);
  void
  writeToHelper(struct iovec* iov, size_t numIov, folly::StringPiece context);
  template <size_t N>
  void writeToHelper(
      std::array<struct iovec, N>& iov,
      folly::StringPiece context) {
    writeToHelper(iov.data(), iov.size(), context);
  }

  SpawnedProcess helper_;
  EdenStatsPtr const stats_;
  ImporterOptions options_;

  /**
   * The input and output file descriptors to the helper subprocess.
   */
  FileDescriptor helperIn_;
  FileDescriptor helperOut_;
};

class HgImporterError : public std::exception {
 public:
  explicit HgImporterError(std::string&& str) : message_(std::move(str)) {}

  const char* what() const noexcept override {
    return message_.c_str();
  }

 private:
  std::string message_;
};

/**
 * A helper class that manages an HgImporter and recreates it after any error
 * communicating with hg debugedenimporthelper.
 *
 * Because HgImporter is thread-bound, HgImporterManager is also thread-bound.
 */
class HgImporterManager : public Importer {
 public:
  HgImporterManager(
      AbsolutePathPiece repoPath,
      EdenStatsPtr,
      std::shared_ptr<StructuredLogger> logger,
      std::optional<AbsolutePath> importHelperScript = std::nullopt);

 private:
  template <typename Fn>
  auto retryOnError(Fn&& fn, FetchMiss::MissType missType);

  HgImporter* getImporter();
  void resetHgImporter(const std::exception& ex);

  std::unique_ptr<HgImporter> importer_;

  const AbsolutePath repoPath_;
  std::string repoName_;
  EdenStatsPtr const stats_;
  std::shared_ptr<StructuredLogger> logger_;
  const std::optional<AbsolutePath> importHelperScript_;
};

} // namespace facebook::eden
