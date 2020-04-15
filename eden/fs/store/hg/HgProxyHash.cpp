/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/hg/HgProxyHash.h"

#include <folly/futures/Future.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <folly/logging/xlog.h>

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

HgProxyHash::HgProxyHash(
    LocalStore* store,
    Hash edenBlobHash,
    StringPiece context) {
  // Read the path name and file rev hash
  auto infoResult = store->get(KeySpace::HgProxyHashFamily, edenBlobHash);
  if (!infoResult.isValid()) {
    XLOG(ERR) << "received unknown mercurial proxy hash "
              << edenBlobHash.toString() << " in " << context;
    // Fall through and let infoResult.extractValue() throw
  }

  value_ = infoResult.extractValue();
  validate(edenBlobHash);
}

folly::Future<std::vector<HgProxyHash>> HgProxyHash::getBatch(
    LocalStore* store,
    const std::vector<Hash>& blobHashes) {
  auto hashCopies = std::make_shared<std::vector<Hash>>(blobHashes);
  std::vector<folly::ByteRange> byteRanges;
  for (auto& hash : *hashCopies) {
    byteRanges.push_back(hash.getBytes());
  }
  return store->getBatch(KeySpace::HgProxyHashFamily, byteRanges)
      .thenValue([blobHashes = hashCopies](std::vector<StoreResult>&& data) {
        std::vector<HgProxyHash> results;

        for (size_t i = 0; i < blobHashes->size(); ++i) {
          results.emplace_back(HgProxyHash{
              blobHashes->at(i), data[i], "prefetchFiles getBatch"});
        }

        return results;
      });
}

Hash HgProxyHash::store(
    RelativePathPiece path,
    Hash hgRevHash,
    LocalStore::WriteBatch* writeBatch) {
  auto computedPair = prepareToStore(path, hgRevHash);
  HgProxyHash::store(computedPair, writeBatch);
  return computedPair.first;
}

std::pair<Hash, IOBuf> HgProxyHash::prepareToStore(
    RelativePathPiece path,
    Hash hgRevHash) {
  // Serialize the (path, hgRevHash) tuple into a buffer.
  auto buf = serialize(path, hgRevHash);

  // Compute the hash of the serialized buffer
  ByteRange serializedInfo = buf.coalesce();
  auto edenBlobHash = Hash::sha1(serializedInfo);

  return std::make_pair(edenBlobHash, std::move(buf));
}

void HgProxyHash::store(
    const std::pair<Hash, IOBuf>& computedPair,
    LocalStore::WriteBatch* writeBatch) {
  writeBatch->put(
      KeySpace::HgProxyHashFamily,
      computedPair.first,
      // Note that this depends on prepareToStore() having called
      // buf.coalesce()!
      ByteRange(computedPair.second.data(), computedPair.second.length()));
}

HgProxyHash::HgProxyHash(
    Hash edenBlobHash,
    StoreResult& infoResult,
    StringPiece context) {
  if (!infoResult.isValid()) {
    XLOG(ERR) << "received unknown mercurial proxy hash "
              << edenBlobHash.toString() << " in " << context;
    // Fall through and let infoResult.extractValue() throw
  }

  value_ = infoResult.extractValue();
  validate(edenBlobHash);
}

IOBuf HgProxyHash::serialize(RelativePathPiece path, Hash hgRevHash) {
  // We serialize the data as <hash_bytes><path_length><path>
  //
  // The path_length is stored as a big-endian uint32_t.
  auto pathStr = path.stringPiece();
  IOBuf buf(IOBuf::CREATE, Hash::RAW_SIZE + sizeof(uint32_t) + pathStr.size());
  Appender appender(&buf, 0);
  appender.push(hgRevHash.getBytes());
  appender.writeBE<uint32_t>(pathStr.size());
  appender.push(pathStr);

  return buf;
}

RelativePathPiece HgProxyHash::path() const {
  DCHECK_GE(value_.size(), Hash::RAW_SIZE + sizeof(uint32_t));
  StringPiece data{value_.data(), value_.size()};
  data.advance(Hash::RAW_SIZE + sizeof(uint32_t));
  return RelativePathPiece{data};
}

Hash HgProxyHash::revHash() const {
  DCHECK_GE(value_.size(), Hash::RAW_SIZE);
  return Hash{ByteRange{StringPiece{value_.data(), Hash::RAW_SIZE}}};
}

void HgProxyHash::validate(Hash edenBlobHash) {
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
}
} // namespace eden
} // namespace facebook
