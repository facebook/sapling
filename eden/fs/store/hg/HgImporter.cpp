/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "eden/fs/store/hg/HgImporter.h"

#include <boost/filesystem/operations.hpp>
#include <boost/filesystem/path.hpp>
#include <folly/Conv.h>
#include <folly/FileUtil.h>
#include <folly/container/Array.h>
#include <folly/dynamic.h>
#include <folly/experimental/EnvUtil.h>
#include <folly/futures/Future.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <folly/json.h>
#include <folly/lang/Bits.h>
#include <folly/logging/xlog.h>
#include <gflags/gflags.h>
#include <glog/logging.h>
#ifndef _WIN32
#include <unistd.h>
#else
#include "eden/fs/win/utils/Pipe.h" // @manual
#include "eden/fs/win/utils/Subprocess.h" // @manual
#include "eden/fs/win/utils/WinError.h" // @manual
#endif

#include <mutex>

#include "eden/fs/eden-config.h"
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/fs/store/hg/HgImportPyError.h"
#include "eden/fs/store/hg/HgManifestImporter.h"
#include "eden/fs/store/hg/HgProxyHash.h"
#include "eden/fs/tracing/EdenStats.h"
#include "eden/fs/utils/PathFuncs.h"
#include "eden/fs/utils/TimeUtil.h"

// Needed for MissingKeyError
#include "edenscm/hgext/extlib/cstore/uniondatapackstore.h" // @manual=//scm/hg:datapack

using folly::ByteRange;
using folly::Endian;
using folly::IOBuf;
using folly::StringPiece;
#ifndef _WIN32
using folly::Subprocess;
#else
using facebook::eden::Pipe;
using facebook::eden::Subprocess;
#endif
using folly::io::Appender;
using folly::io::Cursor;
using std::make_unique;
using std::string;
using std::unique_ptr;
using KeySpace = facebook::eden::LocalStore::KeySpace;

DEFINE_string(
    hgImportHelper,
    "",
    "The path to the mercurial import helper script");

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

constexpr bool kEnableHgImportSubcommand = true;

DEFINE_bool(
    hgImportUseDebugSubcommand,
    // Once we swing through releasing some changes in debugedenimporthelper
    // we can make this default to true everywhere
    kEnableHgImportSubcommand,
    "Use `hg debugedenimporthelper` rather than hgImportHelper");

DEFINE_string(
    hgPythonPath,
    "",
    "Value to use for the PYTHONPATH when running mercurial import script. If "
    "this value is non-empty, the existing PYTHONPATH from the environment is "
    "replaced with this value.");

DEFINE_int32(
    hgManifestImportBufferSize,
    256 * 1024 * 1024, // 256MB
    "Buffer size for batching LocalStore writes during hg manifest imports");

namespace {
using namespace facebook::eden;

/**
 * File descriptor number to use for receiving output from the import helper
 * process.
 *
 * This value is rather arbitrary.  It shouldn't be 0, 1, or 2 (stdin, stdout,
 * or stderr, respectively), but other than that anything is probably fine,
 * since the child shouldn't have any FDs open besides these 3 standard FDs
 * when it starts.
 *
 * The only reason we don't simply use the child's stdout is to avoid
 * communication problems if any of the mercurial helper code somehow ends up
 * printing data to stdout.  We don't want arbitrary log message data from
 * mercurial interfering with our normal communication protocol.
 */

constexpr int HELPER_PIPE_FD = 5;

/**
 * Internal helper function for use by getImportHelperPath().
 *
 * Callers should use getImportHelperPath() rather than directly calling this
 * function.
 */
AbsolutePath findImportHelperPath() {
  // If a path was specified on the command line, use that
  if (!FLAGS_hgImportHelper.empty()) {
    return realpath(FLAGS_hgImportHelper);
  }

  const char* argv0 = gflags::GetArgv0();
  if (argv0 == nullptr) {
    throw std::runtime_error(
        "unable to find hg_import_helper.py script: "
        "unable to determine edenfs executable path");
  }

  auto programPath = realpath(argv0);
  XLOG(DBG4) << "edenfs path: " << programPath;
  auto programDir = programPath.dirname();

  auto isHelper = [](const AbsolutePath& path) {
    XLOG(DBG8) << "checking for hg_import_helper at \"" << path << "\"";
    return access(path.value().c_str(), X_OK) == 0;
  };

#ifdef _WIN32

  // TODO EDENWIN: Remove the hardcoded path and use isHelper to find
  AbsolutePath dir{"C:\\eden\\hg_import_helper\\hg_import_helper.exe"};

  return dir;
#else
  // Check in ../../bin/ relative to the directory containing the edenfs binary.
  // This is where we expect to find the helper script in normal
  // deployments.
  auto path = programDir.dirname().dirname() +
      RelativePathPiece{"bin/hg_import_helper.py"};
  if (isHelper(path)) {
    return path;
  }

  // Check in the directory containing the edenfs binary as well.
  // edenfs and hg_import_helper.py used to be installed in the same directory
  // in the past.  We don't normally expect them to be kept in the same
  // directory any more, but continue searching here just in case.  This may
  // also make it easier to move hg_import_helper.py in the future if we ever
  // want to do so.  (However we will likely just switch to invoking
  // "hg debugedenimporthelper" instead.)
  path = programDir + PathComponentPiece{"hg_import_helper.py"};
  if (isHelper(path)) {
    return path;
  }

  // Now check in all parent directories of the directory containing our
  // binary.  This is where we will find the helper program if we are running
  // from the build output directory in a source code repository.
  AbsolutePathPiece dir = programDir;
  RelativePathPiece helperPath{"eden/fs/store/hg/hg_import_helper.py"};
  while (true) {
    path = dir + helperPath;
    if (isHelper(path)) {
      return path;
    }
    auto parent = dir.dirname();
    if (parent == dir) {
      throw std::runtime_error("unable to find hg_import_helper.py script");
    }
    dir = parent;
  }
#endif
}

/**
 * Get the path to the hg_import_helper.py script.
 *
 * This function is thread-safe and caches the result once we have found
 * the  helper script once.
 */
AbsolutePath getImportHelperPath() {
  // C++11 guarantees that this static initialization will be thread-safe, and
  // if findImportHelperPath() throws it will retry initialization the next
  // time getImportHelperPath() is called.
  static AbsolutePath helperPath = findImportHelperPath();
  return helperPath;
}

#ifndef _WIN32
std::string findInPath(folly::StringPiece executable) {
  auto path = getenv("PATH");
  if (!path) {
    throw std::runtime_error(folly::to<std::string>(
        "unable to resolve ", executable, " in PATH because PATH is not set"));
  }
  std::vector<folly::StringPiece> dirs;
  folly::split(":", path, dirs);

  for (auto& dir : dirs) {
    auto candidate = folly::to<std::string>(dir, "/", executable);
    if (access(candidate.c_str(), X_OK) == 0) {
      return candidate;
    }
  }

  throw std::runtime_error(folly::to<std::string>(
      "unable to resolve ", executable, " in PATH ", path));
}
#endif

} // unnamed namespace

namespace facebook {
namespace eden {

class HgImporterEofError : public HgImporterError {
 public:
  using HgImporterError::HgImporterError;
};

HgImporter::HgImporter(
    AbsolutePathPiece repoPath,
    LocalStore* store,
    std::shared_ptr<HgImporterThreadStats> stats,
    std::optional<AbsolutePath> importHelperScript)
    : repoPath_{repoPath}, store_{store}, stats_{std::move(stats)} {
  std::vector<string> cmd;

  // importHelperScript takes precedence if it was specified; this is used
  // primarily in our integration tests.
  if (importHelperScript.has_value()) {
    cmd.push_back(importHelperScript.value().value());
    cmd.push_back(repoPath.value().str());
  } else if (FLAGS_hgImportUseDebugSubcommand) {
    cmd.push_back(FLAGS_hgPath);
    cmd.push_back("debugedenimporthelper");
  } else {
    cmd.push_back(getImportHelperPath().value());
    cmd.push_back(repoPath.value().str());
  }

#ifndef _WIN32
  cmd.push_back("--out-fd");
  cmd.push_back(folly::to<string>(HELPER_PIPE_FD));

  // In the future, it might be better to use some other arbitrary fd for
  // output from the helper process, rather than stdout (just in case anything
  // in the python code ends up printing to stdout).
  Subprocess::Options opts;
  // Send commands to the child on its stdin.
  // Receive output on HELPER_PIPE_FD.
  opts.stdinFd(Subprocess::PIPE).fd(HELPER_PIPE_FD, Subprocess::PIPE_OUT);

  // Ensure that we run the helper process with cwd set to the repo.
  // This is important for `hg debugedenimporthelper` to pick up the
  // correct configuration in the currently available versions of
  // that subcommand.  In particular, without this, the tests may
  // fail when run in our CI environment.
  opts.chdir(repoPath.value().str());

  // If argv[0] isn't an absolute path then we need to search $PATH.
  // Ideally we'd just tell Subprocess to usePath, but it doesn't
  // allow us to do so when we are also overriding the environment.
  if (!boost::filesystem::path(cmd[0]).is_absolute()) {
    cmd[0] = findInPath(cmd[0]);
  }

  auto env = folly::experimental::EnvironmentState::fromCurrentEnvironment();
  if (!FLAGS_hgPythonPath.empty()) {
    env->erase("PYTHONPATH");
    env->emplace("PYTHONPATH", FLAGS_hgPythonPath);
  }
  // HACK(T33686765): Work around LSAN reports for hg_importer_helper.
  (*env)["LSAN_OPTIONS"] = "detect_leaks=0";
  // If we're using `hg debugedenimporthelper`, don't allow the user
  // configuration to change behavior away from the system defaults.
  // This is harmless even if we're using hg_import_helper.py, so
  // it is done unconditionally.
  (*env)["HGPLAIN"] = "1";
  (*env)["CHGDISABLE"] = "1";

  auto envVector = env.toVector();
  helper_ = Subprocess{cmd, opts, nullptr, &envVector};
  SCOPE_FAIL {
    helper_.closeParentFd(STDIN_FILENO);
    helper_.wait();
  };
  helperIn_ = helper_.stdinFd();
  helperOut_ = helper_.parentFd(HELPER_PIPE_FD);
#else

  auto childInPipe = std::make_unique<Pipe>(nullptr, true);
  auto childOutPipe = std::make_unique<Pipe>(nullptr, true);

  if (!SetHandleInformation(childInPipe->writeHandle, HANDLE_FLAG_INHERIT, 0)) {
    throw std::runtime_error("Failed to set the handle attributes");
  }
  if (!SetHandleInformation(childOutPipe->readHandle, HANDLE_FLAG_INHERIT, 0)) {
    throw std::runtime_error("Failed to set the handle attributes");
  }

  cmd.push_back("--out-fd");
  cmd.push_back(folly::to<string>((int)childOutPipe->writeHandle));
  cmd.push_back("--in-fd");
  cmd.push_back(folly::to<string>((int)childInPipe->readHandle));

  helper_.createSubprocess(
      cmd, std::move(childInPipe), std::move(childOutPipe));
  helperIn_ = helper_.childInPipe_->writeHandle;
  helperOut_ = helper_.childOutPipe_->readHandle;

#endif
  options_ = waitForHelperStart();
  XLOG(DBG1) << "hg_import_helper started for repository " << repoPath_;
}

ImporterOptions HgImporter::waitForHelperStart() {
  // Wait for the import helper to send the CMD_STARTED message indicating
  // that it has started successfully.
  ChunkHeader header;
  try {
    header = readChunkHeader(0, "CMD_STARTED");
  } catch (const HgImporterEofError& error) {
    // If we get EOF trying to read the initial response this generally
    // indicates that the import helper exited with an error early on during
    // startup, before it could send us a success or error message.
    //
    // It should have normally printed an error message to stderr in this case,
    // which is normally redirected to our edenfs.log file.
    throw HgImporterError(
        "error starting Mercurial import helper.  "
        "Check edenfs.log for the error messages from the import helper.");
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
  if ((flags & StartFlag::TREEMANIFEST_SUPPORTED) &&
      numTreemanifestPaths == 0) {
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

  return options;
}

HgImporter::~HgImporter() {
  stopHelperProcess();
}

#ifndef _WIN32
folly::ProcessReturnCode HgImporter::debugStopHelperProcess() {
  stopHelperProcess();
  return helper_.returnCode();
}
#endif

void HgImporter::stopHelperProcess() {
#ifndef _WIN32
  if (helper_.returnCode().running()) {
    helper_.closeParentFd(STDIN_FILENO);
    helper_.wait();
  }
#endif
}

Hash HgImporter::importFlatManifest(StringPiece revName) {
  // Send the manifest request to the helper process
  auto requestID = sendManifestRequest(revName);

  auto writeBatch = store_->beginWrite(FLAGS_hgManifestImportBufferSize);
  HgManifestImporter importer(store_, writeBatch.get());
  size_t numPaths = 0;

  auto start = std::chrono::steady_clock::now();
  IOBuf chunkData;
  while (true) {
    // Read the chunk header
    auto header = readChunkHeader(requestID, "CMD_MANIFEST");

    // Allocate a larger chunk buffer if we need to,
    // but prefer to re-use the old buffer if we can.
    if (header.dataLength > chunkData.capacity()) {
      chunkData = IOBuf(IOBuf::CREATE, header.dataLength);
    } else {
      chunkData.clear();
    }

    readFromHelper(
        chunkData.writableTail(),
        header.dataLength,
        "CMD_MANIFEST response body");
    chunkData.append(header.dataLength);

    // Now process the entries in the chunk
    Cursor cursor(&chunkData);
    while (!cursor.isAtEnd()) {
      readManifestEntry(importer, cursor, writeBatch.get());
      ++numPaths;
    }

    if ((header.flags & FLAG_MORE_CHUNKS) == 0) {
      break;
    }
  }

  writeBatch->flush();

  auto computeEnd = std::chrono::steady_clock::now();
  XLOG(DBG2) << "computed trees for " << numPaths << " manifest paths in "
             << durationStr(computeEnd - start);
  auto rootHash = importer.finish();
  auto recordEnd = std::chrono::steady_clock::now();
  XLOG(DBG2) << "recorded trees for " << numPaths << " manifest paths in "
             << durationStr(recordEnd - computeEnd);

  return rootHash;
}

unique_ptr<Blob> HgImporter::importFileContents(Hash blobHash) {
  // Look up the mercurial path and file revision hash,
  // which we need to import the data from mercurial
  HgProxyHash hgInfo(store_, blobHash, "importFileContents");

  XLOG(DBG5) << "requesting file contents of '" << hgInfo.path() << "', "
             << hgInfo.revHash().toString();

  // Ask the import helper process for the file contents
  auto requestID = sendFileRequest(hgInfo.path(), hgInfo.revHash());

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
        hgInfo.path(),
        ", ",
        hgInfo.revHash(),
        ") from hg_import_helper.py is too "
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
        hgInfo.path(),
        ", ",
        hgInfo.revHash(),
        "): bodyLength=",
        bodyLength,
        " responseLength=",
        header.dataLength);
    XLOG(ERR) << msg;
    throw std::runtime_error(std::move(msg));
  }

  XLOG(DBG4) << "imported blob " << blobHash << " (" << hgInfo.path() << ", "
             << hgInfo.revHash() << "); length=" << bodyLength;

  return make_unique<Blob>(blobHash, std::move(buf));
}

void HgImporter::prefetchFiles(
    const std::vector<std::pair<RelativePath, Hash>>& files) {
  auto requestID = sendPrefetchFilesRequest(files);

  // Read the response; throws if there was any error.
  // No payload is returned.
  readChunkHeader(requestID, "CMD_PREFETCH_FILES");
}

void HgImporter::fetchTree(RelativePathPiece path, Hash pathManifestNode) {
  // Ask the hg_import_helper script to fetch data for this tree
  XLOG(DBG1) << "fetching data for tree \"" << path << "\" at manifest node "
             << pathManifestNode;
  auto requestID = sendFetchTreeRequest(path, pathManifestNode);

  ChunkHeader header;
  header = readChunkHeader(requestID, "CMD_FETCH_TREE");

  if (header.dataLength != 0) {
    throw std::runtime_error(folly::to<string>(
        "got unexpected length ",
        header.dataLength,
        " for FETCH_TREE response"));
  }
}

Hash HgImporter::resolveManifestNode(folly::StringPiece revName) {
  auto requestID = sendManifestNodeRequest(revName);

  auto header = readChunkHeader(requestID, "CMD_MANIFEST_NODE_FOR_COMMIT");
  if (header.dataLength != 20) {
    throw std::runtime_error(folly::to<string>(
        "expected a 20-byte hash for the manifest node '",
        revName,
        "' but got data of length ",
        header.dataLength));
  }

  Hash::Storage buffer;
  readFromHelper(
      buffer.data(),
      buffer.size(),
      "CMD_MANIFEST_NODE_FOR_COMMIT response body");
  return Hash(buffer);
}

void HgImporter::readManifestEntry(
    HgManifestImporter& importer,
    folly::io::Cursor& cursor,
    LocalStore::WriteBatch* writeBatch) {
  Hash::Storage hashBuf;
  cursor.pull(hashBuf.data(), hashBuf.size());
  Hash fileRevHash(hashBuf);

  auto sep = cursor.read<char>();
  if (sep != '\t') {
    throw std::runtime_error(folly::to<string>(
        "unexpected separator char: ", static_cast<int>(sep)));
  }
  auto flag = cursor.read<char>();
  if (flag == '\t') {
    flag = ' ';
  } else {
    sep = cursor.read<char>();
    if (sep != '\t') {
      throw std::runtime_error(folly::to<string>(
          "unexpected separator char: ", static_cast<int>(sep)));
    }
  }

  auto pathStr = cursor.readTerminatedString();

  TreeEntryType fileType;
  if (flag == ' ') {
    fileType = TreeEntryType::REGULAR_FILE;
  } else if (flag == 'x') {
    fileType = TreeEntryType::EXECUTABLE_FILE;
  } else if (flag == 'l') {
    fileType = TreeEntryType::SYMLINK;
  } else {
    throw std::runtime_error(folly::to<string>(
        "unsupported file flags for ", pathStr, ": ", static_cast<int>(flag)));
  }

  RelativePathPiece path(pathStr);

  // Generate a blob hash from the mercurial (path, fileRev) information
  auto blobHash = HgProxyHash::store(path, fileRevHash, writeBatch);

  auto entry = TreeEntry(blobHash, path.basename().value(), fileType);
  importer.processEntry(path.dirname(), std::move(entry));
}

HgImporter::ChunkHeader HgImporter::readChunkHeader(
    TransactionID txnID,
    StringPiece cmdName) {
  ChunkHeader header;
  readFromHelper(&header, sizeof(header), "response header");

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
    auto err = HgImporterError(
        "received unexpected transaction ID (",
        header.requestID,
        " != ",
        txnID,
        ") when reading ",
        cmdName,
        " response");
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
#if defined(EDEN_HAVE_STATS)
  stats_->manifest.addValue(1);
#endif

  auto txnID = nextRequestID_++;
  ChunkHeader header;
  header.command = Endian::big<uint32_t>(CMD_MANIFEST);
  header.requestID = Endian::big<uint32_t>(txnID);
  header.flags = 0;
  header.dataLength = Endian::big<uint32_t>(revName.size());

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
#if defined(EDEN_HAVE_STATS)
  stats_->manifestNodeForCommit.addValue(1);
#endif

  auto txnID = nextRequestID_++;
  ChunkHeader header;
  header.command = Endian::big<uint32_t>(CMD_MANIFEST_NODE_FOR_COMMIT);
  header.requestID = Endian::big<uint32_t>(txnID);
  header.flags = 0;
  header.dataLength = Endian::big<uint32_t>(revName.size());

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
    Hash revHash) {
#if defined(EDEN_HAVE_STATS)
  stats_->catFile.addValue(1);
#endif

  auto txnID = nextRequestID_++;
  ChunkHeader header;
  header.command = Endian::big<uint32_t>(CMD_CAT_FILE);
  header.requestID = Endian::big<uint32_t>(txnID);
  header.flags = 0;
  StringPiece pathStr = path.stringPiece();
  header.dataLength = Endian::big<uint32_t>(Hash::RAW_SIZE + pathStr.size());

  std::array<struct iovec, 3> iov;
  iov[0].iov_base = &header;
  iov[0].iov_len = sizeof(header);
  iov[1].iov_base = const_cast<uint8_t*>(revHash.getBytes().data());
  iov[1].iov_len = Hash::RAW_SIZE;
  iov[2].iov_base = const_cast<char*>(pathStr.data());
  iov[2].iov_len = pathStr.size();
  writeToHelper(iov, "CMD_CAT_FILE");

  return txnID;
}

HgImporter::TransactionID HgImporter::sendPrefetchFilesRequest(
    const std::vector<std::pair<RelativePath, Hash>>& files) {
#if defined(EDEN_HAVE_STATS)
  stats_->prefetchFiles.addValue(1);
#endif

  auto txnID = nextRequestID_++;
  ChunkHeader header;
  header.command = Endian::big<uint32_t>(CMD_PREFETCH_FILES);
  header.requestID = Endian::big<uint32_t>(txnID);
  header.flags = 0;

  // Compute the length of the body
  size_t dataLength = sizeof(uint32_t);
  for (const auto& pair : files) {
    dataLength += sizeof(uint32_t) + pair.first.stringPiece().size() +
        (Hash::RAW_SIZE * 2);
  }
  if (dataLength > std::numeric_limits<uint32_t>::max()) {
    throw std::runtime_error(
        folly::to<string>("prefetch files request is too large: ", dataLength));
  }
  header.dataLength = Endian::big<uint32_t>(dataLength);

  // Serialize the body.
  // We serialize all of the filename lengths first, then all of the strings and
  // hashes later.  This is purely to make it easier to deserialize in python
  // using the struct module.
  //
  // The hashes are serialized as hex since that is how the python code needs
  // them.
  IOBuf buf(IOBuf::CREATE, dataLength);
  Appender appender(&buf, 0);
  appender.writeBE<uint32_t>(files.size());
  for (const auto& pair : files) {
    auto fileName = pair.first.stringPiece();
    appender.writeBE<uint32_t>(fileName.size());
  }
  for (const auto& pair : files) {
    auto fileName = pair.first.stringPiece();
    appender.push(fileName);
    // TODO: It would be nice to have a function that can hexlify the hash
    // data directly into the IOBuf without making a copy in a temporary string.
    // This isn't really that big of a deal though.
    appender.push(StringPiece(pair.second.toString()));
  }
  DCHECK_EQ(buf.length(), dataLength);

  std::array<struct iovec, 2> iov;
  iov[0].iov_base = &header;
  iov[0].iov_len = sizeof(header);
  iov[1].iov_base = const_cast<uint8_t*>(buf.data());
  iov[1].iov_len = buf.length();
  writeToHelper(iov, "CMD_PREFETCH_FILES");

  return txnID;
}

HgImporter::TransactionID HgImporter::sendFetchTreeRequest(
    RelativePathPiece path,
    Hash pathManifestNode) {
#if defined(EDEN_HAVE_STATS)
  stats_->fetchTree.addValue(1);
#endif

  auto txnID = nextRequestID_++;
  ChunkHeader header;
  header.command = Endian::big<uint32_t>(CMD_FETCH_TREE);
  header.requestID = Endian::big<uint32_t>(txnID);
  header.flags = 0;
  StringPiece pathStr = path.stringPiece();
  header.dataLength = Endian::big<uint32_t>(Hash::RAW_SIZE + pathStr.size());

  std::array<struct iovec, 3> iov;
  iov[0].iov_base = &header;
  iov[0].iov_len = sizeof(header);
  iov[1].iov_base = const_cast<uint8_t*>(pathManifestNode.getBytes().data());
  iov[1].iov_len = Hash::RAW_SIZE;
  iov[2].iov_base = const_cast<char*>(pathStr.data());
  iov[2].iov_len = pathStr.size();
  writeToHelper(iov, "CMD_FETCH_TREE");

  return txnID;
}

void HgImporter::readFromHelper(void* buf, size_t size, StringPiece context) {
  size_t bytesRead;

#ifdef _WIN32
  try {
    bytesRead = Pipe::read(helperOut_, buf, size);
  } catch (const std::exception& ex) {
    // The Pipe::read() code can throw std::system_error. Translate this to
    // HgImporterError so that the higher-level code will retry on this error.
    HgImporterError importErr(
        "error reading ",
        context,
        " from hg_import_helper.py: ",
        folly::exceptionStr(ex));
    XLOG(ERR) << importErr.what();
    throw importErr;
  }
#else
  auto result = folly::readFull(helperOut_, buf, size);
  if (result < 0) {
    HgImporterError err(
        "error reading ",
        context,
        " from hg_import_helper.py: ",
        folly::errnoStr(errno));
    XLOG(ERR) << err.what();
    throw err;
  }
  bytesRead = static_cast<size_t>(result);
#endif
  if (bytesRead != size) {
    // The helper process closed the pipe early.
    // This generally means that it exited.
    HgImporterEofError err(
        "received unexpected EOF from hg_import_helper.py after ",
        bytesRead,
        " bytes while reading ",
        context);
    XLOG(ERR) << err.what();
    throw err;
  }
}

void HgImporter::writeToHelper(
    struct iovec* iov,
    size_t numIov,
    StringPiece context) {
#ifdef _WIN32
  try {
    auto result = Pipe::writeiov(helperIn_, iov, numIov);
  } catch (const std::exception& ex) {
    // The Pipe::read() code can throw std::system_error.  Translate this to
    // HgImporterError so that the higher-level code will retry on this error.
    HgImporterError importErr(
        "error writing ",
        context,
        " to hg_import_helper.py: ",
        folly::exceptionStr(ex));
    XLOG(ERR) << importErr.what();
    throw importErr;
  }
#else
  auto result = folly::writevFull(helperIn_, iov, numIov);
  if (result < 0) {
    HgImporterError err(
        "error writing ",
        context,
        " to hg_import_helper.py: ",
        folly::errnoStr(errno));
    XLOG(ERR) << err.what();
    throw err;
  }
  // writevFull() will always write the full contents or fail, so we don't need
  // to check that the length written matches our input.
#endif
}

const ImporterOptions& HgImporter::getOptions() const {
  return options_;
}

HgImporterManager::HgImporterManager(
    AbsolutePathPiece repoPath,
    LocalStore* store,
    std::shared_ptr<HgImporterThreadStats> stats,
    std::optional<AbsolutePath> importHelperScript)
    : repoPath_{repoPath},
      store_{store},
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

Hash HgImporterManager::importFlatManifest(StringPiece revName) {
  return retryOnError([&](HgImporter* importer) {
    return importer->importFlatManifest(revName);
  });
}

Hash HgImporterManager::resolveManifestNode(StringPiece revName) {
  return retryOnError([&](HgImporter* importer) {
    return importer->resolveManifestNode(revName);
  });
}

unique_ptr<Blob> HgImporterManager::importFileContents(Hash blobHash) {
  return retryOnError([&](HgImporter* importer) {
    return importer->importFileContents(blobHash);
  });
}

void HgImporterManager::prefetchFiles(
    const std::vector<std::pair<RelativePath, Hash>>& files) {
  return retryOnError(
      [&](HgImporter* importer) { return importer->prefetchFiles(files); });
}

void HgImporterManager::fetchTree(
    RelativePathPiece path,
    Hash pathManifestNode) {
  return retryOnError([&](HgImporter* importer) {
    return importer->fetchTree(path, pathManifestNode);
  });
}

HgImporter* HgImporterManager::getImporter() {
  if (!importer_) {
    importer_ =
        make_unique<HgImporter>(repoPath_, store_, stats_, importHelperScript_);
  }
  return importer_.get();
}

void HgImporterManager::resetHgImporter(const std::exception& ex) {
  importer_.reset();
  XLOG(WARN) << "error communicating with hg_import_helper.py: " << ex.what();
}

} // namespace eden
} // namespace facebook
