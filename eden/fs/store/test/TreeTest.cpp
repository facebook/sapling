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
  auto entry1Name = PathComponent{StringPiece{"entry1"}};
  auto entry1 =
      TreeEntry{childHash1, TreeEntryType::REGULAR_FILE, size, childSha1};
  auto entry2Name = PathComponent{StringPiece{"entry2"}};
  auto entry2 = TreeEntry{childHash2, TreeEntryType::REGULAR_FILE};
  Tree::container entries;
  entries.push_back({entry1Name, entry1});
  entries.push_back({entry2Name, entry2});
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
