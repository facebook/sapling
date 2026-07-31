/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/GlobPath.h"

#include <gtest/gtest.h>
#include <thrift/lib/cpp2/protocol/BinaryProtocol.h>
#include <thrift/lib/cpp2/protocol/CompactProtocol.h>
#include <algorithm>

namespace facebook::eden {

namespace {

template <typename Protocol>
void expectSerializedSizesMatchIOBuf(const GlobPath& path) {
  using ProtocolMethods = apache::thrift::detail::pm::protocol_methods<
      apache::thrift::type_class::binary,
      GlobPath,
      apache::thrift::type::binary_t>;

  Protocol protocol;
  auto buf = path.toIOBuf();
  EXPECT_EQ(
      protocol.serializedSizeBinary(buf),
      ProtocolMethods::template serializedSize<false>(protocol, path));
  EXPECT_EQ(
      protocol.serializedSizeZCBinary(buf),
      ProtocolMethods::template serializedSize<true>(protocol, path));
}

} // namespace

TEST(GlobPathTest, buildsPathFromSharedDirectoryAndBasename) {
  GlobPathBuilder builder;
  auto dir = builder.makeDir("foo"_relpath);
  dir = builder.childDir(dir, "bar"_pc);
  GlobPath path = builder.makePath(dir, PathComponent{"baz.txt"});

  EXPECT_EQ("foo/bar", path.dir());
  EXPECT_EQ("baz.txt", path.basename());
  EXPECT_EQ("foo/bar/baz.txt", path.asString());
}

TEST(GlobPathTest, buildsRootLevelPath) {
  GlobPath path{RelativePathPiece{"file.txt"}};

  EXPECT_TRUE(path.dir().empty());
  EXPECT_EQ("file.txt", path.basename());
  EXPECT_EQ("file.txt", path.asString());

  auto buf = path.toIOBuf();
  EXPECT_FALSE(buf.isChained());
  EXPECT_EQ("file.txt", buf.moveToFbString().toStdString());
}

TEST(GlobPathTest, ownsExistingFullPathWithoutSplittingStorage) {
  GlobPath path{std::string{"foo/bar/a_long_filename.txt"}};

  EXPECT_EQ("foo/bar", path.dir());
  EXPECT_EQ("a_long_filename.txt", path.basename());
  EXPECT_EQ("foo/bar/a_long_filename.txt", path.asString());

  auto buf = path.toIOBuf();
  EXPECT_FALSE(buf.isChained());
  EXPECT_EQ("foo/bar/a_long_filename.txt", buf.moveToFbString().toStdString());

  GlobPath shared{
      GlobPath::makeDir("foo/bar"_relpath), "a_long_filename.txt"_pc};
  EXPECT_EQ(path, shared);
}

TEST(GlobPathTest, movesOwnedFbStringStorage) {
  folly::fbstring storage{
      "foo/bar/a_filename_long_enough_to_require_allocated_storage.txt"};
  const auto* data = storage.data();
  GlobPath path{std::move(storage)};

  auto result = std::move(path).intoFbString();

  EXPECT_EQ(data, result.data());
  EXPECT_EQ(
      "foo/bar/a_filename_long_enough_to_require_allocated_storage.txt",
      result);
}

TEST(GlobPathTest, comparesAsFullPath) {
  std::vector<GlobPath> paths;
  paths.emplace_back(RelativePathPiece{"foo/b"});
  paths.emplace_back(RelativePathPiece{"foo/a"});
  paths.emplace_back(RelativePathPiece{"foo/a/b"});
  paths.emplace_back(RelativePathPiece{"bar/z"});

  std::sort(paths.begin(), paths.end());

  std::vector<std::string> sorted;
  sorted.reserve(paths.size());
  for (const auto& path : paths) {
    sorted.emplace_back(path.asString());
  }

  const std::vector<std::string> expected{
      "bar/z",
      "foo/a",
      "foo/a/b",
      "foo/b",
  };
  EXPECT_EQ(expected, sorted);
}

TEST(GlobPathTest, serializesAsChainedBuffer) {
  auto dir = GlobPath::makeDir("foo/bar"_relpath);
  GlobPath path{dir, "baz.txt"_pc};

  auto buf = path.toIOBuf();

  EXPECT_TRUE(buf.isChained());
  EXPECT_EQ(path.size(), buf.computeChainDataLength());
  EXPECT_EQ("foo/bar/baz.txt", buf.moveToFbString().toStdString());
}

TEST(GlobPathTest, computesSerializedSizeFromPathLength) {
  GlobPath path{GlobPath::makeDir("foo/bar"_relpath), "baz.txt"_pc};
  const std::string longDir(folly::IOBufQueue::kMaxPackCopy + 1, 'a');
  GlobPath longPath{
      GlobPath::makeDir(RelativePathPiece{longDir}), "baz.txt"_pc};

  expectSerializedSizesMatchIOBuf<apache::thrift::CompactProtocolWriter>(path);
  expectSerializedSizesMatchIOBuf<apache::thrift::BinaryProtocolWriter>(path);
  expectSerializedSizesMatchIOBuf<apache::thrift::CompactProtocolWriter>(
      longPath);
  expectSerializedSizesMatchIOBuf<apache::thrift::BinaryProtocolWriter>(
      longPath);
}

TEST(GlobPathTest, roundTripsThroughCompactProtocol) {
  using ProtocolMethods = apache::thrift::detail::pm::protocol_methods<
      apache::thrift::type_class::binary,
      GlobPath,
      apache::thrift::type::binary_t>;

  GlobPath path{GlobPath::makeDir("foo/bar"_relpath), "baz.txt"_pc};
  folly::IOBufQueue queue;
  apache::thrift::CompactProtocolWriter writer;
  writer.setOutput(&queue);

  const auto size = ProtocolMethods::serializedSize<false>(writer, path);
  const auto zeroCopySize = ProtocolMethods::serializedSize<true>(writer, path);
  const auto written = ProtocolMethods::write(writer, path);
  EXPECT_GE(size, written);
  EXPECT_GE(zeroCopySize, written);

  auto serialized = queue.move();
  apache::thrift::CompactProtocolReader reader;
  reader.setInput(serialized.get());
  GlobPath decoded;
  ProtocolMethods::read(reader, decoded);

  EXPECT_EQ(path, decoded);
}

TEST(GlobPathTest, deserializesPathWithoutAdditionalValidation) {
  using ProtocolMethods = apache::thrift::detail::pm::protocol_methods<
      apache::thrift::type_class::binary,
      GlobPath,
      apache::thrift::type::binary_t>;

  constexpr folly::StringPiece path{"/foo/../bar/"};
  folly::IOBufQueue queue;
  apache::thrift::CompactProtocolWriter writer;
  writer.setOutput(&queue);
  writer.writeBinary(path);

  auto serialized = queue.move();
  apache::thrift::CompactProtocolReader reader;
  reader.setInput(serialized.get());
  GlobPath decoded;
  ProtocolMethods::read(reader, decoded);

  EXPECT_EQ(path, decoded.asString());
}

} // namespace facebook::eden
