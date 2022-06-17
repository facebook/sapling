/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/hg/HgImporter.h"

#include <boost/filesystem/operations.hpp>
#include <boost/filesystem/path.hpp>
#include <folly/Conv.h>
#include <folly/FileUtil.h>
#include <folly/Utility.h>
#include <folly/container/Array.h>
#include <folly/dynamic.h>
#include <folly/experimental/EnvUtil.h>
#include <folly/futures/Future.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <folly/json.h>
#include <folly/lang/Bits.h>
#include <folly/logging/xlog.h>
#include <folly/portability/GFlags.h>
#ifndef _WIN32
#include <folly/portability/Unistd.h>
#endif

#include <mutex>

#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/store/hg/HgImportPyError.h"
#include "eden/fs/store/hg/HgProxyHash.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/utils/PathFuncs.h"
#include "eden/fs/utils/SpawnedProcess.h"

using folly::Endian;
using folly::IOBuf;
using folly::StringPiece;
using folly::io::Appender;
using folly::io::Cursor;
using std::make_unique;
using std::string;
using std::unique_ptr;

#ifdef _WIN32
// We will use the known path to HG executable instead of searching in the
// path. This would make sure we are picking the right mercurial. In future
// we should find a chef config to lookup the path.

DEFINE_string(
    hgPath,
    "C:\\tools\\hg\\hg.real.exe",
    "The path to the mercurial executable");
#else
DEFINE_string(hgPath, "hg.real", "The path to the mercurial executable");
#endif

DEFINE_string(
    hgPythonPath,
    "",
    "Value to use for the PYTHONPATH when running mercurial import script. If "
    "this value is non-empty, the existing PYTHONPATH from the environment is "
    "replaced with this value.");

namespace facebook::eden {

class HgImporterEofError : public HgImporterError {
 public:
  using HgImporterError::HgImporterError;
};

HgImporter::HgImporter(
    AbsolutePathPiece repoPath,
    std::shared_ptr<EdenStats> stats,
    std::optional<AbsolutePath> importHelperScript)
    : repoPath_{repoPath}, stats_{std::move(stats)} {
  std::vector<string> cmd;

  // importHelperScript takes precedence if it was specified; this is used
  // primarily in our integration tests.
  if (importHelperScript.has_value()) {
    cmd.push_back(importHelperScript.value().value());
    cmd.push_back(repoPath.value().str());
  } else {
    cmd.push_back(FLAGS_hgPath);
    cmd.push_back("debugedenimporthelper");
  }

  SpawnedProcess::Options opts;

  opts.nullStdin();

  // Send commands to the child on this pipe
  Pipe childInPipe;
  auto inFd = opts.inheritDescriptor(std::move(childInPipe.read));
  cmd.push_back("--in-fd");
  cmd.push_back(folly::to<string>(inFd));
  helperIn_ = std::move(childInPipe.write);

  // Read responses from this pipe
  Pipe childOutPipe;
  auto outFd = opts.inheritDescriptor(std::move(childOutPipe.write));
  cmd.push_back("--out-fd");
  cmd.push_back(folly::to<string>(outFd));
  helperOut_ = std::move(childOutPipe.read);

  // Ensure that we run the helper process with cwd set to the repo.
  // This is important for `hg debugedenimporthelper` to pick up the
  // correct configuration in the currently available versions of
  // that subcommand.  In particular, without this, the tests may
  // fail when run in our CI environment.
  opts.chdir(AbsolutePathPiece{repoPath.value()});

  if (!FLAGS_hgPythonPath.empty()) {
    opts.environment().set("PYTHONPATH", FLAGS_hgPythonPath);
  }

  // These come from the par file machinery (I think) and can interfere
  // with Mercurial's ability to load dynamic libraries.
  opts.environment().unset("DYLD_LIBRARY_PATH");
  opts.environment().unset("DYLD_INSERT_LIBRARIES");

  // Eden does not control the backing repo's configuration, if it has
  // fsmonitor enabled, it might try to run Watchman, which might
  // cause Watchman to spawn a daemon instance, which might attempt to
  // access the FUSE mount, which might be in the process of starting
  // up. This causes a cross-process deadlock. Thus, in a heavy-handed
  // way, prevent Watchman from ever attempting to spawn an instance.
  opts.environment().set("WATCHMAN_NO_SPAWN", "1");

  cmd.insert(
      cmd.end(),
      {"--config",
       "extensions.fsmonitor=!",
       "--config",
       "extensions.hgevents=!"});

  // HACK(T33686765): Work around LSAN reports for hg_importer_helper.
  opts.environment().set("LSAN_OPTIONS", "detect_leaks=0");

  // If we're using `hg debugedenimporthelper`, don't allow the user
  // configuration to change behavior away from the system defaults.
  opts.environment().set("HGPLAIN", "1");
  opts.environment().set("CHGDISABLE", "1");

  helper_ = SpawnedProcess{cmd, std::move(opts)};
  SCOPE_FAIL {
    helperIn_.close();
    helper_.wait();
  };

  options_ = waitForHelperStart();
  XLOG(DBG1) << "hg_import_helper started for repository " << repoPath_;
}

ImporterOptions HgImporter::waitForHelperStart() {
  // Wait for the import helper to send the CMD_STARTED message indicating
  // that it has started successfully.
  ChunkHeader header;
  try {
    header = readChunkHeader(0, "CMD_STARTED");
  } catch (const HgImporterEofError&) {
    // If we get EOF trying to read the initial response this generally
    // indicates that the import helper exited with an error early on during
    // startup, before it could send us a success or error message.
    //
    // It should have normally printed an error message to stderr in this case,
    // which is normally redirected to our edenfs.log file.
    throw HgImporterError(
        "error starting Mercurial import helper. Run `edenfsctl debug log` to "
        "view the error messages from the import helper.");
  }

  if (header.command != CMD_STARTED) {
    // This normally shouldn't happen.  If an error occurs, the
    // hg_import_helper script should send an error chunk causing
    // readChunkHeader() to throw an exception with the the error message
    // sent back by the script.
    throw std::runtime_error(
        "unexpected start message from hg_import_helper script");
  }

  if (header.dataLength < sizeof(uint32_t)) {
    throw std::runtime_error(
        "missing CMD_STARTED response body from hg_import_helper script");
  }

  IOBuf buf(IOBuf::CREATE, header.dataLength);

  readFromHelper(
      buf.writableTail(), header.dataLength, "CMD_STARTED response body");
  buf.append(header.dataLength);

  Cursor cursor(&buf);
  auto protocolVersion = cursor.readBE<uint32_t>();
  if (protocolVersion != PROTOCOL_VERSION) {
    throw std::runtime_error(folly::to<string>(
        "hg_import_helper protocol version mismatch: edenfs expected ",
        static_cast<uint32_t>(PROTOCOL_VERSION),
        ", hg_import_helper is speaking ",
        protocolVersion));
  }

  ImporterOptions options;

  auto flags = cursor.readBE<uint32_t>();
  auto numTreemanifestPaths = cursor.readBE<uint32_t>();
  if (!(flags & StartFlag::TREEMANIFEST_SUPPORTED)) {
    throw std::runtime_error(
        "hg_import_helper indicated that treemanifest is not supported. "
        "EdenFS requires treemanifest support.");
  }
  if (numTreemanifestPaths == 0) {
    throw std::runtime_error(
        "hg_import_helper indicated that treemanifest "
        "is supported, but provided no store paths");
  }
  for (uint32_t n = 0; n < numTreemanifestPaths; ++n) {
    auto pathLength = cursor.readBE<uint32_t>();
    options.treeManifestPackPaths.push_back(cursor.readFixedString(pathLength));
  }

  if (flags & StartFlag::MONONOKE_SUPPORTED) {
    auto nameLength = cursor.readBE<uint32_t>();
    options.repoName = cursor.readFixedString(nameLength);
  }

  if (!(flags & StartFlag::CAT_TREE_SUPPORTED)) {
    throw std::runtime_error(
        "hg_import_helper indicated that CMD_CAT_TREE is not supported. "
        "As EdenFS requires CMD_CAT_TREE, updating Mercurial is required.");
  }

  return options;
}

HgImporter::~HgImporter() {
  stopHelperProcess();
}

ProcessStatus HgImporter::debugStopHelperProcess() {
  stopHelperProcess();
  return helper_.wait();
}

void HgImporter::stopHelperProcess() {
  if (!helper_.terminated()) {
    helperIn_.close();
    helper_.wait();
  }
}

unique_ptr<Blob> HgImporter::importFileContents(
    RelativePathPiece path,
    Hash20 blobHash) {
  XLOG(DBG5) << "requesting file contents of '" << path << "', "
             << blobHash.toString();

  // Ask the import helper process for the file contents
  auto requestID = sendFileRequest(path, blobHash);

  // Read the response.  The response body contains the file contents,
  // which is exactly what we want to return.
  //
  // Note: For now we expect to receive the entire contents in a single chunk.
  // In the future we might want to consider if it is more efficient to receive
  // the body data in fixed-size chunks, particularly for very large files.
  auto header = readChunkHeader(requestID, "CMD_CAT_FILE");
  if (header.dataLength < sizeof(uint64_t)) {
    auto msg = folly::to<string>(
        "CMD_CAT_FILE response for blob ",
        blobHash,
        " (",
        path,
        ", ",
        blobHash,
        ") from debugedenimporthelper is too "
        "short for body length field: length = ",
        header.dataLength);
    XLOG(ERR) << msg;
    throw std::runtime_error(std::move(msg));
  }
  auto buf = IOBuf(IOBuf::CREATE, header.dataLength);

  readFromHelper(
      buf.writableTail(), header.dataLength, "CMD_CAT_FILE response body");
  buf.append(header.dataLength);

  // The last 8 bytes of the response are the body length.
  // Ensure that this looks correct, and advance the buffer past this data to
  // the start of the actual response body.
  //
  // This data doesn't really need to be present in the response.  It is only
  // here so we can double-check that the response data appears valid.
  buf.trimEnd(sizeof(uint64_t));
  uint64_t bodyLength;
  memcpy(&bodyLength, buf.tail(), sizeof(uint64_t));
  bodyLength = Endian::big(bodyLength);
  if (bodyLength != header.dataLength - sizeof(uint64_t)) {
    auto msg = folly::to<string>(
        "inconsistent body length received when importing blob ",
        blobHash,
        " (",
        path,
        ", ",
        blobHash,
        "): bodyLength=",
        bodyLength,
        " responseLength=",
        header.dataLength);
    XLOG(ERR) << msg;
    throw std::runtime_error(std::move(msg));
  }

  XLOG(DBG4) << "imported blob " << blobHash << " (" << path << ", " << blobHash
             << "); length=" << bodyLength;
  auto blobId =
      ObjectId{blobHash.getBytes()}; // fixme: this is a curious case where we
                                     // create blob using HgId as an Id
  return make_unique<Blob>(blobId, std::move(buf));
}

std::unique_ptr<IOBuf> HgImporter::fetchTree(
    RelativePathPiece path,
    Hash20 pathManifestNode) {
  // Ask the hg_import_helper script to fetch data for this tree
  static constexpr auto getNumRequestsSinceLastLog =
      [](uint64_t& treeRequestsSinceLog) {
        uint64_t numRequests = 0;
        std::swap(numRequests, treeRequestsSinceLog);
        return numRequests;
      };
  XLOG_EVERY_MS(DBG1, 1000)
      << "fetching data for tree \"" << path << "\" at manifest node "
      << pathManifestNode << ". "
      << getNumRequestsSinceLastLog(treeRequestsSinceLog_)
      << " trees fetched since last log";
  treeRequestsSinceLog_++;

  auto requestID = sendFetchTreeRequest(
      CMD_CAT_TREE, path, pathManifestNode, "CMD_CAT_TREE");

  ChunkHeader header;
  header = readChunkHeader(requestID, "CMD_CAT_TREE");

  auto buf = IOBuf::create(header.dataLength);

  readFromHelper(
      buf->writableTail(), header.dataLength, "CMD_CAT_TREE response body");
  buf->append(header.dataLength);

  // The last 8 bytes of the response are the body length.
  // Ensure that this looks correct, and advance the buffer past this data to
  // the start of the actual response body.
  //
  // This data doesn't really need to be present in the response.  It is only
  // here so we can double-check that the response data appears valid.
  buf->trimEnd(sizeof(uint64_t));
  uint64_t bodyLength;
  memcpy(&bodyLength, buf->tail(), sizeof(uint64_t));
  bodyLength = Endian::big(bodyLength);
  if (bodyLength != header.dataLength - sizeof(uint64_t)) {
    auto msg = folly::to<string>(
        "inconsistent body length received when importing tree ",
        pathManifestNode,
        " (",
        path,
        ", ",
        pathManifestNode,
        "): bodyLength=",
        bodyLength,
        " responseLength=",
        header.dataLength);
    XLOG(ERR) << msg;
    throw std::runtime_error(std::move(msg));
  }

  XLOG(DBG4) << "imported tree " << pathManifestNode << " (" << path << ", "
             << pathManifestNode << "); length=" << bodyLength;

  return buf;
}

Hash20 HgImporter::resolveManifestNode(folly::StringPiece revName) {
  auto requestID = sendManifestNodeRequest(revName);

  auto header = readChunkHeader(requestID, "CMD_MANIFEST_NODE_FOR_COMMIT");
  if (header.dataLength != 20) {
    throw std::runtime_error(folly::to<string>(
        "expected a 20-byte hash for the manifest node '",
        revName,
        "' but got data of length ",
        header.dataLength));
  }

  Hash20::Storage buffer;
  readFromHelper(
      buffer.data(),
      folly::to_narrow(buffer.size()),
      "CMD_MANIFEST_NODE_FOR_COMMIT response body");
  return Hash20(buffer);
}

HgImporter::ChunkHeader HgImporter::readChunkHeader(
    TransactionID txnID,
    StringPiece cmdName) {
  ChunkHeader header;
  readFromHelper(&header, folly::to_narrow(sizeof(header)), "response header");

  header.requestID = Endian::big(header.requestID);
  header.command = Endian::big(header.command);
  header.flags = Endian::big(header.flags);
  header.dataLength = Endian::big(header.dataLength);

  // If the header indicates an error, read the error message
  // and throw an exception.
  if ((header.flags & FLAG_ERROR) != 0) {
    readErrorAndThrow(header);
  }

  if (header.requestID != txnID) {
    auto err = HgImporterError(fmt::format(
        FMT_STRING(
            "received unexpected transaction ID ({}) != {}) when reading {} response"),
        header.requestID,
        txnID,
        cmdName));
    XLOG(ERR) << err.what();
    throw err;
  }

  return header;
}

[[noreturn]] void HgImporter::readErrorAndThrow(const ChunkHeader& header) {
  auto buf = IOBuf{IOBuf::CREATE, header.dataLength};
  readFromHelper(buf.writableTail(), header.dataLength, "error response body");
  buf.append(header.dataLength);

  Cursor cursor(&buf);
  auto errorTypeLength = cursor.readBE<uint32_t>();
  StringPiece errorType{cursor.peekBytes().subpiece(0, errorTypeLength)};
  cursor.skip(errorTypeLength);
  auto messageLength = cursor.readBE<uint32_t>();
  StringPiece message{cursor.peekBytes().subpiece(0, messageLength)};
  cursor.skip(messageLength);

  XLOG(WARNING) << "error received from hg helper process: " << errorType
                << ": " << message;
  throw HgImportPyError(errorType, message);
}

HgImporter::TransactionID HgImporter::sendManifestRequest(
    folly::StringPiece revName) {
  stats_->getHgImporterStatsForCurrentThread().manifest.addValue(1);

  auto txnID = nextRequestID_++;
  ChunkHeader header;
  header.command = Endian::big<uint32_t>(CMD_MANIFEST);
  header.requestID = Endian::big<uint32_t>(txnID);
  header.flags = 0;
  header.dataLength = Endian::big<uint32_t>(folly::to_narrow(revName.size()));

  std::array<struct iovec, 2> iov;
  iov[0].iov_base = &header;
  iov[0].iov_len = sizeof(header);
  iov[1].iov_base = const_cast<char*>(revName.data());
  iov[1].iov_len = revName.size();
  writeToHelper(iov, "CMD_MANIFEST");

  return txnID;
}

HgImporter::TransactionID HgImporter::sendManifestNodeRequest(
    folly::StringPiece revName) {
  stats_->getHgImporterStatsForCurrentThread().manifestNodeForCommit.addValue(
      1);

  auto txnID = nextRequestID_++;
  ChunkHeader header;
  header.command = Endian::big<uint32_t>(CMD_MANIFEST_NODE_FOR_COMMIT);
  header.requestID = Endian::big<uint32_t>(txnID);
  header.flags = 0;
  header.dataLength = Endian::big<uint32_t>(folly::to_narrow(revName.size()));

  std::array<struct iovec, 2> iov;
  iov[0].iov_base = &header;
  iov[0].iov_len = sizeof(header);
  iov[1].iov_base = const_cast<char*>(revName.data());
  iov[1].iov_len = revName.size();
  writeToHelper(iov, "CMD_MANIFEST_NODE_FOR_COMMIT");

  return txnID;
}

HgImporter::TransactionID HgImporter::sendFileRequest(
    RelativePathPiece path,
    Hash20 revHash) {
  stats_->getHgImporterStatsForCurrentThread().catFile.addValue(1);

  auto txnID = nextRequestID_++;
  ChunkHeader header;
  header.command = Endian::big<uint32_t>(CMD_CAT_FILE);
  header.requestID = Endian::big<uint32_t>(txnID);
  header.flags = 0;
  StringPiece pathStr = path.stringPiece();
  header.dataLength = Endian::big<uint32_t>(
      folly::to_narrow(Hash20::RAW_SIZE + pathStr.size()));

  std::array<struct iovec, 3> iov;
  iov[0].iov_base = &header;
  iov[0].iov_len = sizeof(header);
  iov[1].iov_base = const_cast<uint8_t*>(revHash.getBytes().data());
  iov[1].iov_len = Hash20::RAW_SIZE;
  iov[2].iov_base = const_cast<char*>(pathStr.data());
  iov[2].iov_len = pathStr.size();
  writeToHelper(iov, "CMD_CAT_FILE");

  return txnID;
}

HgImporter::TransactionID HgImporter::sendFetchTreeRequest(
    CommandType cmd,
    RelativePathPiece path,
    Hash20 pathManifestNode,
    StringPiece context) {
  stats_->getHgImporterStatsForCurrentThread().fetchTree.addValue(1);

  auto txnID = nextRequestID_++;
  ChunkHeader header;
  header.command = Endian::big<uint32_t>(cmd);
  header.requestID = Endian::big<uint32_t>(txnID);
  header.flags = 0;
  StringPiece pathStr = path.stringPiece();
  header.dataLength = Endian::big<uint32_t>(
      folly::to_narrow(Hash20::RAW_SIZE + pathStr.size()));

  std::array<struct iovec, 3> iov;
  iov[0].iov_base = &header;
  iov[0].iov_len = sizeof(header);
  iov[1].iov_base = const_cast<uint8_t*>(pathManifestNode.getBytes().data());
  iov[1].iov_len = Hash20::RAW_SIZE;
  iov[2].iov_base = const_cast<char*>(pathStr.data());
  iov[2].iov_len = pathStr.size();
  writeToHelper(iov, context);

  return txnID;
}

void HgImporter::readFromHelper(void* buf, uint32_t size, StringPiece context) {
  size_t bytesRead;

  auto result = helperOut_.readFull(buf, size);

  if (result.hasException()) {
    HgImporterError err(fmt::format(
        FMT_STRING("error reading {} from debugedenimporthelper: {}"),
        context,
        folly::exceptionStr(result.exception()).c_str()));
    XLOG(ERR) << err.what();
    throw err;
  }
  bytesRead = static_cast<size_t>(result.value());
  if (bytesRead != size) {
    // The helper process closed the pipe early.
    // This generally means that it exited.
    HgImporterEofError err(fmt::format(
        FMT_STRING(
            "received unexpected EOF from debugedenimporthelper after {} bytes while reading {}"),
        bytesRead,
        context));
    XLOG(ERR) << err.what();
    throw err;
  }
}

void HgImporter::writeToHelper(
    struct iovec* iov,
    size_t numIov,
    StringPiece context) {
  auto result = helperIn_.writevFull(iov, numIov);
  if (result.hasException()) {
    HgImporterError err(fmt::format(
        FMT_STRING("error writing {} to debugedenimporthelper: {}"),
        context,
        folly::exceptionStr(result.exception()).c_str()));
    XLOG(ERR) << err.what();
    throw err;
  }
  // writevFull() will always write the full contents or fail, so we don't need
  // to check that the length written matches our input.
}

const ImporterOptions& HgImporter::getOptions() const {
  return options_;
}

HgImporterManager::HgImporterManager(
    AbsolutePathPiece repoPath,
    std::shared_ptr<EdenStats> stats,
    std::optional<AbsolutePath> importHelperScript)
    : repoPath_{repoPath},
      stats_{std::move(stats)},
      importHelperScript_{importHelperScript} {}

template <typename Fn>
auto HgImporterManager::retryOnError(Fn&& fn) {
  bool retried = false;

  auto retryableError = [this, &retried](const std::exception& ex) {
    resetHgImporter(ex);
    if (retried) {
      throw;
    } else {
      XLOG(INFO) << "restarting hg_import_helper and retrying operation";
      retried = true;
    }
  };

  while (true) {
    try {
      return fn(getImporter());
    } catch (const HgImportPyError& ex) {
      if (ex.errorType() == "ResetRepoError") {
        // The python code thinks its repository state has gone bad, and
        // is requesting to be restarted
        retryableError(ex);
      } else {
        throw;
      }
    } catch (const HgImporterError& ex) {
      retryableError(ex);
    }
  }
}

Hash20 HgImporterManager::resolveManifestNode(StringPiece revName) {
  return retryOnError([&](HgImporter* importer) {
    return importer->resolveManifestNode(revName);
  });
}

unique_ptr<Blob> HgImporterManager::importFileContents(
    RelativePathPiece path,
    Hash20 blobHash) {
  return retryOnError([=](HgImporter* importer) {
    return importer->importFileContents(path, blobHash);
  });
}

std::unique_ptr<IOBuf> HgImporterManager::fetchTree(
    RelativePathPiece path,
    Hash20 pathManifestNode) {
  return retryOnError([&](HgImporter* importer) {
    return importer->fetchTree(path, pathManifestNode);
  });
}

HgImporter* HgImporterManager::getImporter() {
  if (!importer_) {
    importer_ = make_unique<HgImporter>(repoPath_, stats_, importHelperScript_);
  }
  return importer_.get();
}

void HgImporterManager::resetHgImporter(const std::exception& ex) {
  importer_.reset();
  XLOG(WARN) << "error communicating with debugedenimporthelper: " << ex.what();
}

} // namespace facebook::eden
