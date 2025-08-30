/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/model/git/GitTree.h"

#include <gtest/gtest.h>
#include <string>

#include <folly/String.h>

#include "eden/fs/model/Hash.h"
#include "eden/fs/model/TestOps.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/TreeEntry.h"

using namespace facebook::eden;
using folly::IOBuf;
using folly::StringPiece;
using folly::io::Appender;
using std::string;

string toBinaryHash(const string& hex);

TEST(GitTree, testDeserialize) {
  // This is an id for a tree object in https://github.com/facebook/nuclide.git
  // You can verify its contents with:
  // `git cat-file -p 8e073e366ed82de6465d1209d3f07da7eebabb93`.
  string treeId("8e073e366ed82de6465d1209d3f07da7eebabb93");
  ObjectId id = ObjectId::fromHex(treeId);

  auto gitTreeObject = folly::to<string>(
      string("tree 424\x00", 9),

      string("100644 .babelrc\x00", 16),
      toBinaryHash("3a8f8eb91101860fd8484154885838bf322964d0"),

      string("100644 .flowconfig\x00", 19),
      toBinaryHash("3610882f48696cc7ca0835929511c9db70acbec6"),

      string("100644 README.md\x00", 17),
      toBinaryHash("c5f15617ed29cd35964dc197a7960aeaedf2c2d5"),

      string("40000 lib\x00", 10),
      toBinaryHash("e95798e17f694c227b7a8441cc5c7dae50a187d0"),

      string("100755 nuclide-start-server\x00", 28),
      toBinaryHash("006babcf5734d028098961c6f4b6b6719656924b"),

      string("100644 package.json\x00", 20),
      toBinaryHash("582591e0f0d92cb63a85156e39abd43ebf103edc"),

      string("40000 scripts\x00", 14),
      toBinaryHash("e664fd28e60a0da25739fdf732f412ab3e91d1e1"),

      string("100644 services-3.json\x00", 23),
      toBinaryHash("3ead3c6cd723f4867bef4444ba18e6ffbf0f711a"),

      string("100644 services-config.json\x00", 28),
      toBinaryHash("bbc8e67499b7f3e1ea850eeda1253be7da5c9199"),

      string("40000 spec\x00", 11),
      toBinaryHash("3bae53a99d080dd851f78e36eb343320091a3d57"),

      string("100644 xdebug.ini\x00", 18),
      toBinaryHash("9ed5bbccd1b9b0077561d14c0130dc086ab27e04"));

  auto tree = deserializeGitTree(id, StringPiece(gitTreeObject));
  EXPECT_EQ(11, tree->size());
  EXPECT_EQ(treeId, Hash20::sha1(StringPiece{gitTreeObject}).toString())
      << "SHA-1 of contents should match key";

  // Ordinary, non-executable file.
  auto babelrc = *tree->find(".babelrc"_pc);
  EXPECT_EQ(
      ObjectId::fromHex("3a8f8eb91101860fd8484154885838bf322964d0"),
      babelrc.second.getObjectId());
  EXPECT_EQ(".babelrc", babelrc.first);
  EXPECT_EQ(false, babelrc.second.isTree());
  EXPECT_EQ(
      facebook::eden::TreeEntryType::REGULAR_FILE, babelrc.second.getType());

  // Executable file.
  auto nuclideStartServer = *tree->find("nuclide-start-server"_pc);
  EXPECT_EQ(
      ObjectId::fromHex("006babcf5734d028098961c6f4b6b6719656924b"),
      nuclideStartServer.second.getObjectId());
  EXPECT_EQ("nuclide-start-server", nuclideStartServer.first);
  EXPECT_EQ(false, nuclideStartServer.second.isTree());
  // TODO: T66590035
#ifndef _WIN32
  EXPECT_EQ(
      facebook::eden::TreeEntryType::EXECUTABLE_FILE,
      nuclideStartServer.second.getType());
#endif

  // Directory.
  auto lib = *tree->find("lib"_pc);
  EXPECT_EQ(
      ObjectId::fromHex("e95798e17f694c227b7a8441cc5c7dae50a187d0"),
      lib.second.getObjectId());
  EXPECT_EQ("lib", lib.first);
  EXPECT_EQ(true, lib.second.isTree());
  EXPECT_EQ(facebook::eden::TreeEntryType::TREE, lib.second.getType());

  // lab sorts before lib but is not present in the Tree, so ensure that
  // we don't get an entry back here
  EXPECT_EQ(tree->end(), tree->find("lab"_pc));
}

TEST(GitTree, testDeserializeWithSymlink) {
  // This is an id for a tree object in https://github.com/atom/atom.git
  // You can verify its contents with:
  // `git cat-file -p 013b7865a6da317bc8d82c7225eb93615f1b1eca`.
  string treeId("013b7865a6da317bc8d82c7225eb93615f1b1eca");
  ObjectId id = ObjectId::fromHex(treeId);

  auto gitTreeObject = folly::to<string>(
      string("tree 223\x00", 9),

      string("100644 README.md\x00", 17),
      toBinaryHash("c66788d87933862e2111a86304b705dd90bbd427"),

      string("100644 apm-rest-api.md\x00", 23),
      toBinaryHash("a3c8e5c25e5523322f0ea490173dbdc1d844aefb"),

      string("40000 build-instructions\x00", 25),
      toBinaryHash("de0b8287939193ed239834991be65b96cbfc4508"),

      string("100644 contributing-to-packages.md\x00", 35),
      toBinaryHash("4576635ff317960be244b1c4adfe2a6eb2eb024d"),

      string("120000 contributing.md\x00", 23),
      toBinaryHash("44fcc63439371c8c829df00eec6aedbdc4d0e4cd"));

  auto tree = deserializeGitTree(id, StringPiece(gitTreeObject));
  EXPECT_EQ(5, tree->size());
  EXPECT_EQ(treeId, Hash20::sha1(StringPiece{gitTreeObject}).toString())
      << "SHA-1 of contents should match key";

  // Ordinary, non-executable file.
  auto contributing = *tree->find("contributing.md"_pc);
  EXPECT_EQ(
      ObjectId::fromHex("44fcc63439371c8c829df00eec6aedbdc4d0e4cd"),
      contributing.second.getObjectId());
  EXPECT_EQ("contributing.md", contributing.first);
  EXPECT_EQ(false, contributing.second.isTree());

  // TODO: T66590035
#ifndef _WIN32
  EXPECT_EQ(
      facebook::eden::TreeEntryType::SYMLINK, contributing.second.getType());
#endif
}

TEST(GitTree, deserializeEmpty) {
  // Test deserializing the empty tree
  auto data = StringPiece("tree 0\x00", 7);
  auto tree = deserializeGitTree(ObjectId::sha1(data), data);
  EXPECT_EQ(0, tree->size());
}

TEST(GitTree, testBadDeserialize) {
  ObjectId zero = ObjectId::fromHex("0000000000000000000000000000000000000000");
  // Partial header
  // @lint-ignore SPELL
  EXPECT_ANY_THROW(deserializeGitTree(zero, StringPiece("tre")));
  EXPECT_ANY_THROW(deserializeGitTree(zero, StringPiece("tree ")));
  EXPECT_ANY_THROW(deserializeGitTree(zero, StringPiece("tree 123")));

  // Length too long
  IOBuf buf(IOBuf::CREATE, 1024);
  auto a = Appender(&buf, 1024);
  a.push(StringPiece("tree 123"));
  a.write<uint8_t>(0);
  EXPECT_ANY_THROW(deserializeGitTree(zero, &buf));

  // Truncated after an entry mode
  buf.clear();
  a = Appender(&buf, 1024);
  a.push(StringPiece("tree 6"));
  a.write<uint8_t>(0);
  a.push(StringPiece("100644"));
  EXPECT_ANY_THROW(deserializeGitTree(zero, &buf));

  // Truncated with no nul byte after the name
  buf.clear();
  a = Appender(&buf, 1024);
  a.push(StringPiece("tree 22"));
  a.write<uint8_t>(0);
  a.push(StringPiece("100644 apm-rest-api.md"));
  EXPECT_ANY_THROW(deserializeGitTree(zero, &buf));

  // Truncated before entry id
  buf.clear();
  a = Appender(&buf, 1024);
  a.push(StringPiece("tree 23"));
  a.write<uint8_t>(0);
  a.push(StringPiece("100644 apm-rest-api.md"));
  a.write<uint8_t>(0);
  EXPECT_ANY_THROW(deserializeGitTree(zero, &buf));

  // Non-octal digit in the mode
  buf.clear();
  a = Appender(&buf, 1024);
  a.push(StringPiece("tree 43"));
  a.write<uint8_t>(0);
  a.push(StringPiece("100694 apm-rest-api.md"));
  a.write<uint8_t>(0);
  a.push(Hash20("a3c8e5c25e5523322f0ea490173dbdc1d844aefb").getBytes());
  EXPECT_ANY_THROW(deserializeGitTree(zero, &buf));

  // Trailing nul byte
  buf.clear();
  a = Appender(&buf, 1024);
  a.push(StringPiece("tree 44"));
  a.write<uint8_t>(0);
  a.push(StringPiece("100644 apm-rest-api.md"));
  a.write<uint8_t>(0);
  a.push(Hash20("a3c8e5c25e5523322f0ea490173dbdc1d844aefb").getBytes());
  a.write<uint8_t>(0);
  EXPECT_ANY_THROW(deserializeGitTree(zero, &buf));
}

string toBinaryHash(const string& hex) {
  string bytes;
  folly::unhexlify(hex, bytes);
  return bytes;
}
