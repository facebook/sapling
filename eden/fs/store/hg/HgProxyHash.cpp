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
using KeySpace = facebook::eden::LocalStore::KeySpace;

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
  parseValue(edenBlobHash);
}

folly::Future<std::vector<std::pair<RelativePath, Hash>>> HgProxyHash::getBatch(
    LocalStore* store,
    const std::vector<Hash>& blobHashes) {
  auto hashCopies = std::make_shared<std::vector<Hash>>(blobHashes);
  std::vector<folly::ByteRange> byteRanges;
  for (auto& hash : *hashCopies) {
    byteRanges.push_back(hash.getBytes());
  }
  return store->getBatch(KeySpace::HgProxyHashFamily, byteRanges)
      .thenValue([blobHashes = hashCopies](std::vector<StoreResult>&& data) {
        std::vector<std::pair<RelativePath, Hash>> results;

        for (size_t i = 0; i < blobHashes->size(); ++i) {
          HgProxyHash hgInfo(
              blobHashes->at(i), data[i], "prefetchFiles getBatch");

          results.emplace_back(hgInfo.path().copy(), hgInfo.revHash());
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
  parseValue(edenBlobHash);
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

void HgProxyHash::parseValue(Hash edenBlobHash) {
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

} // namespace eden
} // namespace facebook
