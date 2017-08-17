/*
 *  Copyright (c) 2016-present, Facebook, Inc.
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
#include <folly/experimental/EnvUtil.h>
#include <folly/experimental/logging/xlog.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <gflags/gflags.h>
#include <glog/logging.h>
#include <unistd.h>
#include <mutex>

#include "HgManifestImporter.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/fs/store/StoreResult.h"
#include "eden/fs/utils/PathFuncs.h"
#include "eden/fs/utils/TimeUtil.h"

#include "scm/hgext/cstore/uniondatapackstore.h"
#include "scm/hgext/ctreemanifest/treemanifest.h"

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

DEFINE_string(
    hgPythonPath,
    "",
    "Value to use for the PYTHONPATH when running mercurial import script. If "
    "this value is non-empty, the existing PYTHONPATH from the environment is "
    "replaced with this value.");

DEFINE_bool(
    use_hg_tree_manifest,
    false, // Disabled for now until we are able to handle MissingKeyError
    "Attempt to import mercurial trees using treemanifest");
DEFINE_bool(
    hg_fetch_missing_trees,
    true,
    "Set this parameter to \"no\" to disable fetching missing treemanifest "
    "trees from the remote mercurial server.  This is generally only useful "
    "for testing/debugging purposes");

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
 * HgProxyHash manages mercurial (path, revHash) data in the LocalStore.
 *
 * Mercurial doesn't really have a blob hash the same way eden and git do.
 * Instead, mercurial file revision hashes are always relative to a specific
 * path.  To use the data in eden, we need to create a blob hash that we can
 * use instead.
 *
 * To do so, we hash the (path, revHash) tuple, and use this hash as the blob
 * hash in eden.  We store the eden_blob_hash --> (path, hgRevHash) mapping
 * in the LocalStore.  The HgProxyHash class helps store and retrieve these
 * mappings.
 */
struct HgProxyHash {
 public:
  /**
   * Load HgProxyHash data for the given eden blob hash from the LocalStore.
   */
  HgProxyHash(LocalStore* store, Hash edenBlobHash) {
    // Read the path name and file rev hash
    auto infoResult = store->get(StringPiece(getBlobKey(edenBlobHash)));
    if (!infoResult.isValid()) {
      XLOG(ERR) << "received unknown mercurial proxy hash "
                << edenBlobHash.toString();
      // Fall through and let infoResult.extractValue() throw
    }

    value_ = infoResult.extractValue();
    parseValue(edenBlobHash);
  }

  ~HgProxyHash() {}

  const RelativePathPiece& path() const {
    return path_;
  }

  const Hash& revHash() const {
    return revHash_;
  }

  /**
   * Store HgProxyHash data in the LocalStore.
   *
   * Returns an eden blob hash that can be used to retrieve the data later
   * (using the HgProxyHash constructor defined above).
   */
  static Hash store(LocalStore* store, RelativePathPiece path, Hash hgRevHash) {
    auto computedPair = prepareToStore(path, hgRevHash);
    HgProxyHash::store(store, computedPair);
    return computedPair.first;
  }

  /**
   * Compute the proxy hash information, but do not store it.
   *
   * This is useful when you need the proxy hash but don't want to commit
   * the data until after you have written an associated data item.
   * Returns the proxy hash and the data that should be written;
   * the caller is responsible for passing the pair to the HgProxyHash::store()
   * method below at the appropriate time.
   */
  static std::pair<Hash, IOBuf> prepareToStore(
      RelativePathPiece path,
      Hash hgRevHash) {
    // Serialize the (path, hgRevHash) tuple into a buffer.
    auto buf = serialize(path, hgRevHash);

    // Compute the hash of the serialized buffer
    ByteRange serializedInfo = buf.coalesce();
    auto edenBlobHash = Hash::sha1(serializedInfo);

    return std::make_pair(edenBlobHash, std::move(buf));
  }

  /**
   * Store precomputed proxy hash information.
   * Stores the data computed by prepareToStore().
   */
  static void store(
      LocalStore* store,
      const std::pair<Hash, IOBuf>& computedPair) {
    store->put(
        StringPiece(getBlobKey(computedPair.first)),
        // Note that this depends on prepareToStore() having called
        // buf.coalesce()!
        ByteRange(computedPair.second.data(), computedPair.second.length()));
  }

 private:
  // Not movable or copyable.
  // path_ points into value_, and would need to be updated after
  // copying/moving the data.  Since no-one needs to copy or move HgProxyHash
  // objects, we don't implement this for now.
  HgProxyHash(const HgProxyHash&) = delete;
  HgProxyHash& operator=(const HgProxyHash&) = delete;
  HgProxyHash(HgProxyHash&&) = delete;
  HgProxyHash& operator=(HgProxyHash&&) = delete;

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
      XLOG(ERR) << msg;
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
      XLOG(ERR) << msg;
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

  // Check in the same directory as the edenfs binary.
  // This is where we expect to find the helper script in normal
  // deployments.
  PathComponentPiece helperName{"hg_import_helper.py"};
  auto path = programDir + helperName;
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

} // unnamed namespace

namespace facebook {
namespace eden {

HgImporter::HgImporter(AbsolutePathPiece repoPath, LocalStore* store)
    : store_(store) {
  auto importHelper = getImportHelperPath();
  std::vector<string> cmd = {
      importHelper.value().toStdString(),
      repoPath.value().str(),
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
  auto env = folly::experimental::EnvironmentState::fromCurrentEnvironment();
  if (!FLAGS_hgPythonPath.empty()) {
    env->erase("PYTHONPATH");
    env->emplace("PYTHONPATH", FLAGS_hgPythonPath);
  }
  auto envVector = env.toVector();
  helper_ = Subprocess{cmd, opts, nullptr, &envVector};
  SCOPE_FAIL {
    helper_.closeParentFd(STDIN_FILENO);
    helper_.wait();
  };
  helperIn_ = helper_.stdinFd();
  helperOut_ = helper_.parentFd(HELPER_PIPE_FD);

  waitForHelperStart();
  XLOG(DBG1) << "hg_import_helper started for repository " << repoPath
             << ": treemanifest=" << (unionStore_ ? "yes" : "no");
}

void HgImporter::waitForHelperStart() {
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

  if (header.dataLength < sizeof(uint32_t)) {
    throw std::runtime_error(
        "missing CMD_STARTED response body from hg_import_helper script");
  }

  IOBuf buf(IOBuf::CREATE, header.dataLength);
  folly::readFull(helperOut_, buf.writableTail(), header.dataLength);
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

  auto flags = cursor.readBE<uint32_t>();
  auto numTreemanifestPaths = cursor.readBE<uint32_t>();
  std::vector<DatapackStore*> storePtrs;
  for (uint32_t n = 0; n < numTreemanifestPaths; ++n) {
    auto pathLength = cursor.readBE<uint32_t>();
    auto path = cursor.readFixedString(pathLength);
    XLOG(DBG5) << "treemanifest pack path: " << path;
    dataPackStores_.emplace_back(std::make_unique<DatapackStore>(path));
    storePtrs.emplace_back(dataPackStores_.back().get());
  }

  if (flags & StartFlag::TREEMANIFEST_SUPPORTED) {
    if (dataPackStores_.empty()) {
      throw std::runtime_error(
          "hg_import_helper indicated that treemanifest "
          "is supported, but provided no store paths");
    }
    unionStore_ = std::make_unique<UnionDatapackStore>(storePtrs);
  }
}

HgImporter::~HgImporter() {
  helper_.closeParentFd(STDIN_FILENO);
  helper_.wait();
}

std::unique_ptr<Tree> HgImporter::importTree(const Hash& id) {
  HgProxyHash pathInfo(store_, id);
  return importTreeImpl(
      pathInfo.revHash(), // this is really the manifest node
      id,
      pathInfo.path());
}

std::unique_ptr<Tree> HgImporter::importTreeImpl(
    const Hash& manifestNode,
    const Hash& edenTreeID,
    RelativePathPiece path) {
  XLOG(DBG6) << "importing tree " << edenTreeID << ": hg manifest "
             << manifestNode << " for path \"" << path << "\"";
  ConstantStringRef content;
  try {
    content = unionStore_->get(
        Key(path.stringPiece().data(),
            path.stringPiece().size(),
            (const char*)manifestNode.getBytes().data(),
            manifestNode.getBytes().size()));
  } catch (const MissingKeyError& ex) {
    // Data for this tree was not present locally.
    // Fall through and fetch the data from the server below.
    if (!FLAGS_hg_fetch_missing_trees) {
      throw;
    }
  }

  if (!content.content()) {
    // Ask the hg_import_helper script to fetch data for this tree
    XLOG(DBG1) << "fetching data for tree \"" << path << "\" at manifest node "
               << manifestNode;
    sendFetchTreeRequest(path, manifestNode);
    auto header = readChunkHeader();
    if (header.dataLength != 0) {
      throw std::runtime_error(folly::to<string>(
          "got unexpected length ",
          header.dataLength,
          " for FETCH_TREE response"));
    }

    // Now try loading it again.
    content = unionStore_->get(
        Key(path.stringPiece().data(),
            path.stringPiece().size(),
            (const char*)manifestNode.getBytes().data(),
            manifestNode.getBytes().size()));
  }

  if (!content.content()) {
    // This generally shouldn't happen: the UnionDatapackStore throws on error
    // instead of returning null.  We're checking simply due to an abundance of
    // caution.
    throw std::domain_error(folly::to<string>(
        "HgImporter::importTree received null tree from mercurial store for ",
        path,
        ", ID ",
        manifestNode.toString()));
  }

  Manifest manifest(
      content, reinterpret_cast<const char*>(manifestNode.getBytes().data()));
  std::vector<TreeEntry> entries;

  auto iter = manifest.getIterator();
  while (!iter.isfinished()) {
    auto* entry = iter.currentvalue();

    // The node is the hex string representation of the hash, but
    // it is not NUL terminated!
    StringPiece node(entry->node, 40);
    Hash entryHash(node);

    StringPiece entryName(entry->filename, entry->filenamelen);

    FileType fileType;
    uint8_t ownerPermissions;

    StringPiece entryFlag;
    if (entry->flag) {
      // entry->flag is a char* but is unfortunately not nul terminated.
      // All known flag values are currently only a single character, and there
      // are never any multi-character flags.
      entryFlag.assign(entry->flag, entry->flag + 1);
    }

    XLOG(DBG9) << "tree: " << manifestNode << " " << entryName
               << " node: " << node << " flag: " << entryFlag;

    if (entry->isdirectory()) {
      fileType = FileType::DIRECTORY;
      ownerPermissions = 0b111;
    } else if (entry->flag) {
      switch (*entry->flag) {
        case 'x':
          fileType = FileType::REGULAR_FILE;
          ownerPermissions = 0b111;
          break;
        case 'l':
          fileType = FileType::SYMLINK;
          ownerPermissions = 0b111;
          break;
        default:
          throw std::runtime_error(folly::to<string>(
              "unsupported file flags for ",
              path,
              "/",
              entryName,
              ": ",
              entryFlag));
      }
    } else {
      fileType = FileType::REGULAR_FILE;
      ownerPermissions = 0b110;
    }

    auto proxyHash = HgProxyHash::store(
        store_, path + RelativePathPiece(entryName), entryHash);

    entries.emplace_back(proxyHash, entryName, fileType, ownerPermissions);

    iter.next();
  }

  auto tree = std::make_unique<Tree>(std::move(entries), edenTreeID);
  auto serialized = store_->serializeTree(tree.get());
  store_->put(edenTreeID, serialized.second.coalesce());
  return tree;
}

Hash HgImporter::importManifest(StringPiece revName) {
  if (FLAGS_use_hg_tree_manifest) {
    try {
      return importTreeManifest(revName);
    } catch (const MissingKeyError&) {
      // We don't have a tree manifest available for the target rev,
      // so let's fall through to the full flat manifest importer.
    }
  }

  return importFlatManifest(revName);
}

Hash HgImporter::importTreeManifest(StringPiece revName) {
  auto manifestNode = resolveManifestNode(revName);
  XLOG(DBG2) << "revision " << revName << " has manifest node " << manifestNode;

  // Record that we are at the root for this node
  RelativePathPiece path{};
  auto proxyInfo = HgProxyHash::prepareToStore(path, manifestNode);
  auto tree = importTreeImpl(manifestNode, proxyInfo.first, path);
  // Only write the proxy hash value for this once we've imported
  // the root.
  HgProxyHash::store(store_, proxyInfo);

  return tree->getHash();
}

Hash HgImporter::importFlatManifest(StringPiece revName) {
  // Send the manifest request to the helper process
  sendManifestRequest(revName);

  return importFlatManifest(helperOut_, store_);
}

Hash HgImporter::importFlatManifest(int fd, LocalStore* store) {
  HgManifestImporter importer(store);
  size_t numPaths = 0;

  auto start = std::chrono::steady_clock::now();
  IOBuf chunkData;
  while (true) {
    // Read the chunk header
    auto header = readChunkHeader(fd);

    // Allocate a larger chunk buffer if we need to,
    // but prefer to re-use the old buffer if we can.
    if (header.dataLength > chunkData.capacity()) {
      chunkData = IOBuf(IOBuf::CREATE, header.dataLength);
    } else {
      chunkData.clear();
    }
    folly::readFull(fd, chunkData.writableTail(), header.dataLength);
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
  auto computeEnd = std::chrono::steady_clock::now();
  XLOG(DBG2) << "computed trees for " << numPaths << " manifest paths in "
             << durationStr(computeEnd - start);
  auto rootHash = importer.finish();
  auto recordEnd = std::chrono::steady_clock::now();
  XLOG(DBG2) << "recorded trees for " << numPaths << " manifest paths in "
             << durationStr(recordEnd - computeEnd);

  return rootHash;
}

IOBuf HgImporter::importFileContents(Hash blobHash) {
  // Look up the mercurial path and file revision hash,
  // which we need to import the data from mercurial
  HgProxyHash hgInfo(store_, blobHash);
  XLOG(DBG5) << "requesting file contents of '" << hgInfo.path() << "', "
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

Hash HgImporter::resolveManifestNode(folly::StringPiece revName) {
  sendManifestNodeRequest(revName);

  auto header = readChunkHeader();
  if (header.dataLength != 20) {
    throw std::runtime_error(folly::to<string>(
        "expected a 20-byte hash for the manifest node, "
        "but got data of length ",
        header.dataLength));
  }

  Hash::Storage buffer;
  folly::readFull(helperOut_, &buffer[0], buffer.size());

  return Hash(buffer);
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
  auto blobHash =
      HgProxyHash::store(importer.getLocalStore(), path, fileRevHash);

  auto entry =
      TreeEntry(blobHash, path.basename().value(), fileType, ownerPermissions);
  importer.processEntry(path.dirname(), std::move(entry));
}

HgImporter::ChunkHeader HgImporter::readChunkHeader(int fd) {
  ChunkHeader header;
  folly::readFull(fd, &header, sizeof(header));
  header.requestID = Endian::big(header.requestID);
  header.command = Endian::big(header.command);
  header.flags = Endian::big(header.flags);
  header.dataLength = Endian::big(header.dataLength);

  // If the header indicates an error, read the error message
  // and throw an exception.
  if ((header.flags & FLAG_ERROR) != 0) {
    std::vector<char> errMsg(header.dataLength);
    folly::readFull(fd, &errMsg.front(), header.dataLength);
    string errStr(&errMsg.front(), errMsg.size());
    XLOG(WARNING) << "error received from hg helper process: " << errStr;
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

void HgImporter::sendManifestNodeRequest(folly::StringPiece revName) {
  ChunkHeader header;
  header.command = Endian::big<uint32_t>(CMD_MANIFEST_NODE_FOR_COMMIT);
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

void HgImporter::sendFetchTreeRequest(
    RelativePathPiece path,
    Hash pathManifestNode) {
  ChunkHeader header;
  header.command = Endian::big<uint32_t>(CMD_FETCH_TREE);
  header.requestID = Endian::big<uint32_t>(nextRequestID_++);
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
  folly::writevFull(helperIn_, iov.data(), iov.size());
}
}
} // facebook::eden
