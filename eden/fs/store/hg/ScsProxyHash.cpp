/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/hg/ScsProxyHash.h"

#include <optional>
#include <string>

#include <folly/futures/Future.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <folly/logging/xlog.h>

#include "eden/fs/model/Hash.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/fs/store/StoreResult.h"

using folly::ByteRange;
using folly::Endian;
using folly::IOBuf;
using folly::StringPiece;
using folly::io::Appender;
using std::string;

namespace facebook {
namespace eden {

ScsProxyHash::ScsProxyHash(std::string value) : value_(value) {}

std::optional<ScsProxyHash>
ScsProxyHash::load(LocalStore* store, Hash edenBlobHash, StringPiece context) {
  // Read the path name and commit hash
  auto infoResult = store->get(KeySpace::ScsProxyHashFamily, edenBlobHash);
  if (!infoResult.isValid()) {
    XLOG(DBG3) << "scs proxy hash received unknown mercurial proxy hash "
               << edenBlobHash.toString() << " in " << context;
    return std::nullopt;
  }

  return ScsProxyHash(infoResult.extractValue());
}

void ScsProxyHash::store(
    Hash edenBlobHash,
    RelativePathPiece path,
    Hash commitHash,
    LocalStore::WriteBatch* writeBatch) {
  auto buf = prepareToStore(path, commitHash);

  writeBatch->put(
      KeySpace::ScsProxyHashFamily,
      edenBlobHash,
      // Note that this depends on prepareToStore(..) having called
      // buf.coalesce()!
      ByteRange(buf.data(), buf.length()));
}

folly::IOBuf ScsProxyHash::prepareToStore(
    RelativePathPiece path,
    Hash commitHash) {
  // Serialize the (path, hgRevHash) tuple into a buffer.
  auto buf = serialize(path, commitHash);
  buf.coalesce();
  return buf;
}

folly::IOBuf ScsProxyHash::serialize(RelativePathPiece path, Hash commitHash) {
  // We serialize the data as <hash_bytes><path_length><path>
  //
  // The path_length is stored as a big-endian uint32_t.
  auto pathStr = path.stringPiece();
  IOBuf buf(IOBuf::CREATE, Hash::RAW_SIZE + sizeof(uint32_t) + pathStr.size());
  Appender appender(&buf, 0);
  appender.push(commitHash.getBytes());
  appender.writeBE<uint32_t>(pathStr.size());
  appender.push(pathStr);

  return buf;
}

Hash ScsProxyHash::commitHash() const {
  DCHECK_GE(value_.size(), Hash::RAW_SIZE);
  return Hash{ByteRange{StringPiece{value_.data(), Hash::RAW_SIZE}}};
}

RelativePathPiece ScsProxyHash::path() const {
  DCHECK_GE(value_.size(), Hash::RAW_SIZE + sizeof(uint32_t));
  StringPiece data{value_.data(), value_.size()};
  data.advance(Hash::RAW_SIZE + sizeof(uint32_t));
  return RelativePathPiece{data};
}

} // namespace eden
} // namespace facebook
