/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "HgImporter.h"

#include <boost/filesystem/operations.hpp>
#include <boost/filesystem/path.hpp>
#include <folly/Array.h>
#include <folly/Bits.h>
#include <folly/Conv.h>
#include <folly/FileUtil.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <gflags/gflags.h>
#include <glog/logging.h>
#include <unistd.h>
#include <mutex>

#include "HgManifestImporter.h"
#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/fs/store/StoreResult.h"
#include "eden/utils/PathFuncs.h"

using folly::ByteRange;
using folly::Endian;
using folly::io::Appender;
using folly::io::Cursor;
using folly::IOBuf;
using folly::StringPiece;
using folly::Subprocess;
using std::string;

DEFINE_string(
    hgImportHelper,
    "",
    "The path to the mercurial import helper script");

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
 * HgBlobInfo manages mercurial (path, revHash) data in the LocalStore.
 *
 * Mercurial doesn't really have a blob hash the same way eden and git do.
 * Instead, mercurial file revision hashes are always relative to a specific
 * path.  To use the data in eden, we need to create a blob hash that we can
 * use instead.
 *
 * To do so, we hash the (path, revHash) tuple, and use this hash as the blob
 * hash in eden.  We store the eden_blob_hash --> (path, hgRevHash) mapping
 * in the LocalStore.  The HgBlobInfo class helps store and retrieve these
 * mappings.
 */
struct HgBlobInfo {
 public:
  /**
   * Load HgBlobInfo data for the given eden blob hash from the LocalStore.
   */
  HgBlobInfo(LocalStore* store, Hash edenBlobHash) {
    // Read the path name and file rev hash
    auto infoResult = store->get(StringPiece(getBlobKey(edenBlobHash)));
    if (!infoResult.isValid()) {
      LOG(ERROR) << "received unknown mercurial blob hash "
                 << edenBlobHash.toString();
      // Fall through and let infoResult.extractValue() throw
    }

    value_ = infoResult.extractValue();
    parseValue(edenBlobHash);
  }

  ~HgBlobInfo() {}

  const RelativePathPiece& path() const {
    return path_;
  }

  const Hash& revHash() const {
    return revHash_;
  }

  /**
   * Store HgBlobInfo data in the LocalStore.
   *
   * Returns an eden blob hash that can be used to retrieve the data later
   * (using the HgBlobInfo constructor defined above).
   */
  static Hash store(LocalStore* store, RelativePathPiece path, Hash hgRevHash) {
    // Serialize the (path, hgRevHash) tuple into a buffer.
    auto buf = serialize(path, hgRevHash);

    // Compute the hash of the serialized buffer
    ByteRange serializedInfo = buf.coalesce();
    auto edenBlobHash = Hash::sha1(serializedInfo);

    // Save the data in the store
    store->put(StringPiece(getBlobKey(edenBlobHash)), serializedInfo);
    return edenBlobHash;
  }

 private:
  // Not movable or copyable.
  // path_ points into value_, and would need to be updated after
  // copying/moving the data.  Since no-one needs to copy or move HgBlobInfo
  // objects, we don't implement this for now.
  HgBlobInfo(const HgBlobInfo&) = delete;
  HgBlobInfo& operator=(const HgBlobInfo&) = delete;
  HgBlobInfo(HgBlobInfo&&) = delete;
  HgBlobInfo& operator=(HgBlobInfo&&) = delete;

  static std::string getBlobKey(Hash edenBlobHash) {
    // TODO: Use a RocksDB column family for this rather than having to
    // use a key suffix.
    auto key = StringPiece(edenBlobHash.getBytes()).str();
    key.append("hgx");
    return key;
  }

  /**
   * Serialize the (path, hgRevHash) data into a buffer that will be stored in
   * the LocalStore.
   */
  static IOBuf serialize(RelativePathPiece path, Hash hgRevHash) {
    // We serialize the data as <hash_bytes><path_length><path>
    //
    // The path_length is stored as a big-endian uint32_t.
    auto pathStr = path.stringPiece();
    IOBuf buf(
        IOBuf::CREATE, Hash::RAW_SIZE + sizeof(uint32_t) + pathStr.size());
    Appender appender(&buf, 0);
    appender.push(hgRevHash.getBytes());
    appender.writeBE<uint32_t>(pathStr.size());
    appender.push(pathStr);

    return buf;
  }

  /**
   * Parse the serialized data found in value_, and set revHash_ and path_.
   *
   * The value_ member variable should already contain the serialized data,
   * (as returned by serialize()).
   *
   * Note that path_ will be set to a RelativePathPiece pointing into the
   * string data owned by value_.  (This lets us avoid copying the string data
   * out.)
   */
  void parseValue(Hash edenBlobHash) {
    ByteRange infoBytes = StringPiece(value_);
    // Make sure the data is long enough to contain the rev hash and path length
    if (infoBytes.size() < Hash::RAW_SIZE + sizeof(uint32_t)) {
      auto msg = folly::to<string>(
          "mercurial blob info data for ",
          edenBlobHash.toString(),
          " is too short (",
          infoBytes.size(),
          " bytes)");
      LOG(ERROR) << msg;
      throw std::length_error(msg);
    }

    // Extract the revHash_
    revHash_ = Hash(infoBytes.subpiece(0, Hash::RAW_SIZE));
    infoBytes.advance(Hash::RAW_SIZE);

    // Extract the path length
    uint32_t pathLength;
    memcpy(&pathLength, infoBytes.data(), sizeof(uint32_t));
    pathLength = Endian::big(pathLength);
    infoBytes.advance(sizeof(uint32_t));
    // Make sure the path length agrees with the length of data remaining
    if (infoBytes.size() != pathLength) {
      auto msg = folly::to<string>(
          "mercurial blob info data for ",
          edenBlobHash.toString(),
          " has inconsistent path length");
      LOG(ERROR) << msg;
      throw std::length_error(msg);
    }

    // Extract the path_
    path_ = RelativePathPiece(StringPiece(infoBytes));
  }

  /**
   * The serialized data.
   */
  std::string value_;
  /**
   * The revision hash.
   */
  Hash revHash_;
  /**
   * The path name.  Note that this points into the serialized value_ data.
   * path_ itself does not own the data it points to.
   */
  RelativePathPiece path_;
};

/**
 * Internal helper function for use by getImportHelperPath().
 *
 * Callers should use getImportHelperPath() rather than directly calling this
 * function.
 */
std::string findImportHelperPath() {
  // If a path was specified on the command line, use that
  if (!FLAGS_hgImportHelper.empty()) {
    return FLAGS_hgImportHelper;
  }

  const char* argv0 = gflags::GetArgv0();
  if (argv0 == nullptr) {
    throw std::runtime_error(
        "unable to find hg_import_helper.py script: "
        "unable to determine edenfs executable path");
  }

  auto programDir = boost::filesystem::absolute(boost::filesystem::path(argv0));
  VLOG(4) << "edenfs path: " << programDir.native();
  programDir.remove_filename();

  auto toCheck = folly::make_array(
      // Check in the same directory as the edenfs binary.
      // This is where we expect to find the helper script in normal
      // deployments.
      programDir / boost::filesystem::path("hg_import_helper.py"),
      // Check relative to the edenfs binary, if we are being run directly
      // from the buck-out directory in a source code repository.
      programDir /
          boost::filesystem::path(
              "../../../../../../eden/fs/store/hg/hg_import_helper.py"));

  for (const auto& path : toCheck) {
    VLOG(5) << "checking for hg_import_helper at \"" << path.native() << "\"";
    boost::filesystem::path normalized;
    try {
      normalized = boost::filesystem::canonical(path);
    } catch (const std::exception& ex) {
      // canonical() only succeeds if the path exists
      continue;
    }
    if (access(normalized.c_str(), X_OK) == 0) {
      return normalized.native();
    }
  }

  throw std::runtime_error("unable to find hg_import_helper.py script");
}

/**
 * Get the path to the hg_import_helper.py script.
 *
 * This function is thread-safe and caches the result once we have found
 * the  helper script once.
 */
std::string getImportHelperPath() {
  // We could use folly::Singleton to store the helper path, but we don't for a
  // couple reasons:
  // - We want to retry finding the helper path on subsequent calls if we fail
  //   finding it the first time.  (If someone has sinced fixed the
  //   installation path for ths script it's nicer to try looking for it
  //   again.)
  // - The Singleton API is slightly awkward for just storing a string with a
  //   custom lookup function.
  //
  // This code should never be accessed during static initialization before
  // main() starts, or during shutdown cleanup, so the we don't really need
  // the extra safety that folly::Singleton provides for those situations.
  static std::mutex helperPathMutex;
  static std::string helperPath;

  std::lock_guard<std::mutex> guard(helperPathMutex);
  if (helperPath.empty()) {
    helperPath = findImportHelperPath();
  }

  return helperPath;
}

} // unnamed namespace

namespace facebook {
namespace eden {

HgImporter::HgImporter(StringPiece repoPath, LocalStore* store)
    : store_(store) {
  std::vector<string> cmd = {
      getImportHelperPath(),
      repoPath.str(),
      "--out-fd",
      folly::to<string>(HELPER_PIPE_FD),
  };

  // In the future, it might be better to use some other arbitrary fd for
  // output from the helper process, rather than stdout (just in case anything
  // in the python code ends up printing to stdout).
  Subprocess::Options opts;
  // Send commands to the child on its stdin.
  // Receive output on HELPER_PIPE_FD.
  opts.stdinFd(Subprocess::PIPE).fd(HELPER_PIPE_FD, Subprocess::PIPE_OUT);
  helper_ = Subprocess{cmd, opts};
  SCOPE_FAIL {
    helper_.closeParentFd(STDIN_FILENO);
    helper_.wait();
  };
  helperIn_ = helper_.stdinFd();
  helperOut_ = helper_.parentFd(HELPER_PIPE_FD);

  // Wait for the import helper to send the CMD_STARTED message indicating
  // that it has started successfully.
  auto header = readChunkHeader();
  if (header.command != CMD_STARTED) {
    // This normally shouldn't happen.  If an error occurs, the
    // hg_import_helper script should send an error chunk causing
    // readChunkHeader() to throw an exception with the the error message
    // sent back by the script.
    throw std::runtime_error(
        "unexpected start message from hg_import_helper script");
  }
}

HgImporter::~HgImporter() {
  helper_.closeParentFd(STDIN_FILENO);
  helper_.wait();
}

Hash HgImporter::importManifest(StringPiece revName) {
  // Send the manifest request to the helper process
  sendManifestRequest(revName);

  HgManifestImporter importer(store_);
  size_t numPaths = 0;

  IOBuf chunkData;
  while (true) {
    // Read the chunk header
    auto header = readChunkHeader();

    // Allocate a larger chunk buffer if we need to,
    // but prefer to re-use the old buffer if we can.
    if (header.dataLength > chunkData.capacity()) {
      chunkData = IOBuf(IOBuf::CREATE, header.dataLength);
    } else {
      chunkData.clear();
    }
    folly::readFull(helperOut_, chunkData.writableTail(), header.dataLength);
    chunkData.append(header.dataLength);

    // Now process the entries in the chunk
    Cursor cursor(&chunkData);
    while (!cursor.isAtEnd()) {
      readManifestEntry(importer, cursor);
      ++numPaths;
    }

    if ((header.flags & FLAG_MORE_CHUNKS) == 0) {
      break;
    }
  }
  auto rootHash = importer.finish();
  VLOG(1) << "processed " << numPaths << " manifest paths";

  return rootHash;
}

IOBuf HgImporter::importFileContents(Hash blobHash) {
  // Look up the mercurial path and file revision hash,
  // which we need to import the data from mercurial
  HgBlobInfo hgInfo(store_, blobHash);
  VLOG(5) << "requesting file contents of '" << hgInfo.path() << "', "
          << hgInfo.revHash().toString();

  // Ask the import helper process for the file contents
  sendFileRequest(hgInfo.path(), hgInfo.revHash());

  // Read the response.  The response body contains the file contents,
  // which is exactly what we want to return.
  //
  // Note: For now we expect to receive the entire contents in a single chunk.
  // In the future we might want to consider if it is more efficient to receive
  // the body data in fixed-size chunks, particularly for very large files.
  auto header = readChunkHeader();
  auto buf = IOBuf(IOBuf::CREATE, header.dataLength);
  folly::readFull(helperOut_, buf.writableTail(), header.dataLength);
  buf.append(header.dataLength);

  return buf;
}

void HgImporter::readManifestEntry(
    HgManifestImporter& importer,
    folly::io::Cursor& cursor) {
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

  FileType fileType;
  uint8_t ownerPermissions;
  if (flag == ' ') {
    fileType = FileType::REGULAR_FILE;
    ownerPermissions = 0b110;
  } else if (flag == 'x') {
    fileType = FileType::REGULAR_FILE;
    ownerPermissions = 0b111;
  } else if (flag == 'l') {
    fileType = FileType::SYMLINK;
    ownerPermissions = 0b111;
  } else {
    throw std::runtime_error(folly::to<string>(
        "unsupported file flags for ", pathStr, ": ", static_cast<int>(flag)));
  }

  RelativePathPiece path(pathStr);

  // Generate a blob hash from the mercurial (path, fileRev) information
  auto blobHash = HgBlobInfo::store(store_, path, fileRevHash);

  auto entry =
      TreeEntry(blobHash, path.basename().value(), fileType, ownerPermissions);
  importer.processEntry(path.dirname(), std::move(entry));
}

HgImporter::ChunkHeader HgImporter::readChunkHeader() {
  ChunkHeader header;
  folly::readFull(helperOut_, &header, sizeof(header));
  header.requestID = Endian::big(header.requestID);
  header.command = Endian::big(header.command);
  header.flags = Endian::big(header.flags);
  header.dataLength = Endian::big(header.dataLength);

  // If the header indicates an error, read the error message
  // and throw an exception.
  if ((header.flags & FLAG_ERROR) != 0) {
    std::vector<char> errMsg(header.dataLength);
    folly::readFull(helperOut_, &errMsg.front(), header.dataLength);
    string errStr(&errMsg.front(), errMsg.size());
    LOG(WARNING) << "error received from hg helper process: " << errStr;
    throw std::runtime_error(errStr);
  }

  return header;
}

void HgImporter::sendManifestRequest(folly::StringPiece revName) {
  ChunkHeader header;
  header.command = Endian::big<uint32_t>(CMD_MANIFEST);
  header.requestID = Endian::big<uint32_t>(nextRequestID_++);
  header.flags = 0;
  header.dataLength = Endian::big<uint32_t>(revName.size());

  std::array<struct iovec, 2> iov;
  iov[0].iov_base = &header;
  iov[0].iov_len = sizeof(header);
  iov[1].iov_base = const_cast<char*>(revName.data());
  iov[1].iov_len = revName.size();
  folly::writevFull(helperIn_, iov.data(), iov.size());
}

void HgImporter::sendFileRequest(RelativePathPiece path, Hash revHash) {
  ChunkHeader header;
  header.command = Endian::big<uint32_t>(CMD_CAT_FILE);
  header.requestID = Endian::big<uint32_t>(nextRequestID_++);
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
  folly::writevFull(helperIn_, iov.data(), iov.size());
}
}
} // facebook::eden
