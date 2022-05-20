/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <stdexcept>
#include <variant>
#include <vector>

#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/KeySpace.h"
#include "eden/fs/store/MemoryLocalStore.h"
#include "eden/fs/store/test/LocalStoreTest.h"

using namespace facebook::eden;
using namespace folly;

TEST_P(LocalStoreTest, testReadAndWriteTree) {
  ObjectId hash{"3a8f8eb91101860fd8484154885838bf322964d0aacc"};
  ObjectId childHash1("8e073e366ed82de6465d1209d3f07da7eebabb93bbdd");
  ObjectId childHash2("8e073e366ed82de6465d1209d3f07da7eebabb939988");

  StringPiece childContents("blah\n");
  auto childSha1 = Hash20::sha1(folly::ByteRange{childContents});
  auto size = childContents.size();
  Tree::container entries{kPathMapDefaultCaseSensitive};
  entries.emplace(
      "entry1"_pc, childHash1, TreeEntryType::REGULAR_FILE, size, childSha1);
  entries.emplace("entry2"_pc, childHash2, TreeEntryType::REGULAR_FILE);
  auto tree = Tree{std::move(entries), hash};

  auto serialized = tree.serialize();
  serialized.coalesce();

  store_->put(
      KeySpace::TreeFamily,
      hash.getBytes(),
      folly::ByteRange(serialized.data(), serialized.length()));

  auto outResult = store_->get(KeySpace::TreeFamily, hash);
  ASSERT_TRUE(outResult.isValid());

  auto outTree = Tree::tryDeserialize(hash, outResult.piece());

  ASSERT_TRUE(outTree);
  EXPECT_EQ(*outTree, tree);
}
