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

#include <folly/Bits.h>
#include <folly/Conv.h>
#include <folly/FileUtil.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <glog/logging.h>

#include "HgManifestImporter.h"
#include "eden/fs/model/TreeEntry.h"
#include "eden/utils/PathFuncs.h"

using folly::Endian;
using folly::io::Cursor;
using folly::IOBuf;
using folly::StringPiece;
using folly::Subprocess;
using std::string;

DEFINE_string(
    hgImportHelper,
    "./eden/fs/importer/hg/hg_import_helper.py",
    "The path to the mercurial import helper script");

namespace facebook {
namespace eden {

HgImporter::HgImporter(StringPiece repoPath, LocalStore* store)
    : store_(store) {
  std::vector<string> cmd = {
      FLAGS_hgImportHelper, repoPath.str(),
  };

  // In the future, it might be better to use some other arbitrary fd for
  // output from the helper process, rather than stdout (just in case anything
  // in the python code ends up printing to stdout).
  Subprocess::Options opts;
  opts.stdin(Subprocess::PIPE).stdout(Subprocess::PIPE);
  helper_ = Subprocess{cmd, opts};

  // TODO: Read some sort of success response back from the helper, to make
  // sure it has started successfully.  For instance, if the repository doesn't
  // exist it will bail out early, and we should catch that.
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
    folly::readFull(
        helper_.stdout(), chunkData.writableTail(), header.dataLength);
    chunkData.append(header.dataLength);

    if ((header.flags & FLAG_ERROR) != 0) {
      auto message = StringPiece(chunkData.coalesce()).str();
      throw std::runtime_error("error importing hg data: " + message);
    }

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

void HgImporter::readManifestEntry(
    HgManifestImporter& importer,
    folly::io::Cursor& cursor) {
  Hash::Storage hashBuf;
  cursor.pull(hashBuf.data(), hashBuf.size());
  Hash hash(hashBuf);

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

  auto entry =
      TreeEntry(hash, path.basename().value(), fileType, ownerPermissions);
  importer.processEntry(path.dirname(), std::move(entry));
}

HgImporter::ChunkHeader HgImporter::readChunkHeader() {
  ChunkHeader header;
  folly::readFull(helper_.stdout(), &header, sizeof(header));
  header.requestID = Endian::big(header.requestID);
  header.command = Endian::big(header.command);
  header.flags = Endian::big(header.flags);
  header.dataLength = Endian::big(header.dataLength);
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
  folly::writevFull(helper_.stdin(), iov.data(), iov.size());
}
}
} // facebook::eden
