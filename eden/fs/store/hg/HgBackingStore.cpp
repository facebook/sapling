/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "HgBackingStore.h"

#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/fs/store/StoreResult.h"

using folly::ByteRange;
using folly::StringPiece;
using std::unique_ptr;

namespace facebook {
namespace eden {

HgBackingStore::HgBackingStore(StringPiece repository, LocalStore* localStore)
    : importer_(folly::construct_in_place, repository, localStore),
      localStore_(localStore) {}

HgBackingStore::~HgBackingStore() {}

unique_ptr<Tree> HgBackingStore::getTree(const Hash& id) {
  // HgBackingStore imports all relevant Tree objects inside getTreeForCommit()
  // We should never have a case where we are asked for a Tree that hasn't
  // already been imported.
  LOG(ERROR) << "HgBackingStore asked for unknown tree ID " << id.toString();
  return nullptr;
}

unique_ptr<Blob> HgBackingStore::getBlob(const Hash& id) {
  // TODO
  return nullptr;
}

unique_ptr<Tree> HgBackingStore::getTreeForCommit(const Hash& commitID) {
  // TODO: We should probably switch to using a RocksDB column family rather
  // than a key suffix here.
  static constexpr StringPiece mappingSuffix{"hgc"};
  std::array<uint8_t, Hash::RAW_SIZE + mappingSuffix.size()> mappingKeyStorage;
  memcpy(mappingKeyStorage.data(), commitID.getBytes().data(), Hash::RAW_SIZE);
  memcpy(
      mappingKeyStorage.data() + Hash::RAW_SIZE, "hgc", mappingSuffix.size());
  ByteRange mappingKey(mappingKeyStorage.data(), mappingKeyStorage.size());

  Hash rootTreeHash;
  auto result = localStore_->get(mappingKey);
  if (result.isValid()) {
    rootTreeHash = Hash{result.bytes()};
    VLOG(5) << "found existing tree " << rootTreeHash.toString()
            << " for mercurial commit " << commitID.toString();
  } else {
    rootTreeHash = importer_->importManifest(commitID.toString());
    VLOG(1) << "imported mercurial commit " << commitID.toString()
            << " as tree " << rootTreeHash.toString();

    localStore_->put(mappingKey, rootTreeHash.getBytes());
  }

  return localStore_->getTree(rootTreeHash);
}
}
} // facebook::eden
