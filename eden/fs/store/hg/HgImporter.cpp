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
#include <folly/portability/Unistd.h>

#include <mutex>

#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/store/hg/HgImportPyError.h"
#include "eden/fs/store/hg/HgProxyHash.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/telemetry/StructuredLogger.h"
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
    EdenStatsPtr stats,
    std::optional<AbsolutePath> importHelperScript)
    : stats_{std::move(stats)} {
  std::vector<string> cmd;

  // importHelperScript takes precedence if it was specified; this is used
  // primarily in our integration tests.
  if (importHelperScript.has_value()) {
    cmd.push_back(importHelperScript.value().value());
    cmd.push_back(repoPath.stringWithoutUNC());
  } else {
    cmd.push_back(FLAGS_hgPath);
    cmd.emplace_back("debugedenimporthelper");
  }

  SpawnedProcess::Options opts;

  opts.nullStdin();

  // Send commands to the child on this pipe
  Pipe childInPipe;
  auto inFd = opts.inheritDescriptor(std::move(childInPipe.read));
  cmd.emplace_back("--in-fd");
  cmd.push_back(folly::to<string>(inFd));
  helperIn_ = std::move(childInPipe.write);

  // Read responses from this pipe
  Pipe childOutPipe;
  auto outFd = opts.inheritDescriptor(std::move(childOutPipe.write));
  cmd.emplace_back("--out-fd");
  cmd.push_back(folly::to<string>(outFd));
  helperOut_ = std::move(childOutPipe.read);

  // Ensure that we run the helper process with cwd set to the repo.
  // This is important for `hg debugedenimporthelper` to pick up the
  // correct configuration in the currently available versions of
  // that subcommand.  In particular, without this, the tests may
  // fail when run in our CI environment.
  opts.chdir(repoPath);

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
       "extensions.hgevents=!",
       "--config",
       "edenapi.max-retry-per-request=0"});

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
  XLOG(DBG1) << "hg_import_helper started for repository " << repoPath;
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
    EdenStatsPtr stats,
    std::shared_ptr<StructuredLogger> logger,
    std::optional<AbsolutePath> importHelperScript)
    : repoPath_{repoPath},
      repoName_(HgImporter(repoPath, stats.copy()).getOptions().repoName),
      stats_{std::move(stats)},
      logger_{std::move(logger)},
      importHelperScript_{importHelperScript} {}

template <typename Fn>
auto HgImporterManager::retryOnError(Fn&& fn, FetchMiss::MissType missType) {
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

  try {
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
  } catch (const std::exception& ex) {
    logger_->logEvent(FetchMiss{
        repoPath_.asString(),
        FetchMiss::HgImporter,
        missType,
        folly::to<string>(ex.what()),
        true});
    throw;
  }
}

HgImporter* HgImporterManager::getImporter() {
  if (!importer_) {
    importer_ =
        make_unique<HgImporter>(repoPath_, stats_.copy(), importHelperScript_);
  }
  return importer_.get();
}

void HgImporterManager::resetHgImporter(const std::exception& ex) {
  importer_.reset();
  XLOG(WARN) << "error communicating with debugedenimporthelper: " << ex.what();
}

} // namespace facebook::eden
