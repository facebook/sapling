/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/chrono/Conv.h>
#include <gmock/gmock.h>
#include <gtest/gtest.h>
#include <scm/mononoke/apiserver/gen-cpp2/MononokeAPIService.h>
#include <scm/mononoke/apiserver/gen-cpp2/MononokeAPIServiceAsyncClient.h>
#include <thrift/lib/cpp2/util/ScopedServerInterfaceThread.h>

#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/mononoke/MononokeThriftBackingStore.h"

using namespace std::chrono_literals;
using facebook::eden::Hash;
using facebook::eden::MononokeThriftBackingStore;

using namespace scm::mononoke::apiserver::thrift;

constexpr folly::Duration kTimeout =
    std::chrono::duration_cast<folly::Duration>(60s);

MononokeBlob makeBlob(std::string content) {
  MononokeBlob blob;
  blob.set_content(std::make_unique<folly::IOBuf>(
      folly::IOBuf::CopyBufferOp::COPY_BUFFER, content));
  return blob;
}

MononokeFile makeFile(
    std::string name,
    std::string node = "0000000000000000000000000000000000000000",
    std::optional<std::string> contentSha1 = std::nullopt,
    std::optional<int64_t> size = std::nullopt) {
  MononokeNodeHash hash;
  hash.set_hash(node);

  MononokeFile file;
  file.set_file_type(MononokeFileType::FILE);
  file.set_hash(hash);
  file.set_name(name);

  if (contentSha1 != std::nullopt) {
    file.set_content_sha1(*contentSha1);
  }

  if (size != std::nullopt) {
    file.set_size(*size);
  }

  return file;
}

MononokeDirectory makeDirectory(std::vector<MononokeFile> files) {
  MononokeDirectory directory;
  directory.set_files(files);
  return directory;
}

MononokeChangeset makeChangeset(
    std::string commitHash,
    std::string manifestHash) {
  MononokeTreeHash manifest;
  manifest.set_hash(manifestHash);

  MononokeChangeset changeset;
  changeset.set_commit_hash(commitHash);
  changeset.set_manifest(std::move(manifest));
  return changeset;
}

class MononokeAPIServiceTestHandler : public MononokeAPIServiceSvIf {
 public:
  folly::Future<std::unique_ptr<MononokeBlob>> future_get_blob(
      std::unique_ptr<MononokeGetBlobParams> params) override {
    return folly::makeFutureWith([&] {
      const auto& hash = params->get_blob_hash().get_hash();
      if (hash == expectedBlobhash_) {
        return std::make_unique<MononokeBlob>(makeBlob(expectedBlob_));
      }
      throw makeException(MononokeAPIExceptionKind::NotFound);
    });
  }

  folly::Future<std::unique_ptr<MononokeDirectory>> future_get_tree(
      std::unique_ptr<MononokeGetTreeParams> params) override {
    const auto& hash = params->get_tree_hash().get_hash();

    if (hash == expectedTreehash_) {
      return folly::makeFuture(
          std::make_unique<MononokeDirectory>(makeDirectory(expectedFiles_)));
    }

    return folly::makeFuture<std::unique_ptr<MononokeDirectory>>(
        makeException(MononokeAPIExceptionKind::NotFound));
  }

  folly::Future<std::unique_ptr<MononokeChangeset>> future_get_changeset(
      std::unique_ptr<MononokeGetChangesetParams> params) override {
    const auto& hash = params->get_revision().get_commit_hash();

    if (hash == expectedChangesetHash_) {
      return folly::makeFuture(std::make_unique<MononokeChangeset>(
          makeChangeset(expectedChangesetHash_, expectedManifest_)));
    }

    return folly::makeFuture<std::unique_ptr<MononokeChangeset>>(
        makeException(MononokeAPIExceptionKind::NotFound));
  }

  void setGetTreeExpectation(
      const std::string& hash,
      const std::vector<MononokeFile>& files) {
    expectedTreehash_ = hash;
    expectedFiles_ = files;
  }

  void setGetBlobExpectation(const std::string& hash, const std::string& blob) {
    expectedBlobhash_ = hash;
    expectedBlob_ = blob;
  }

  void setGetChangesetExpectation(
      const std::string& changesetHash,
      const std::string& manifest) {
    expectedChangesetHash_ = changesetHash;
    expectedManifest_ = manifest;
  }

 private:
  std::string expectedBlobhash_;
  std::string expectedBlob_;

  std::string expectedTreehash_;
  std::vector<MononokeFile> expectedFiles_;

  std::string expectedChangesetHash_;
  std::string expectedManifest_;

  const MononokeAPIException makeException(MononokeAPIExceptionKind&& kind) {
    MononokeAPIException exp;
    exp.set_kind(kind);
    return exp;
  }
};

class MononokeThriftTest : public ::testing::Test {
 protected:
  std::shared_ptr<MononokeAPIServiceTestHandler> handler =
      std::make_shared<MononokeAPIServiceTestHandler>();
  apache::thrift::ScopedServerInterfaceThread runner =
      apache::thrift::ScopedServerInterfaceThread(handler);
  MononokeThriftBackingStore store = MononokeThriftBackingStore(
      runner.newClient<MononokeAPIServiceAsyncClient>(),
      "fbsource",
      &folly::QueuedImmediateExecutor::instance());
};

TEST_F(MononokeThriftTest, getBlob) {
  const std::string blob = "hello";
  const std::string hash = "8888888888888888888888888888888888888888";

  handler->setGetBlobExpectation(hash, blob);

  auto result = store.getBlob(Hash(hash)).get(kTimeout);
  EXPECT_EQ(result->getHash().toString(), hash);

  auto content = result->getContents();
  auto blobString = content.moveToFbString();
  EXPECT_EQ(blobString, blob);
}

TEST_F(MononokeThriftTest, getBlobNotFound) {
  const std::string blob = "hello";
  const std::string hash = "8888888888888888888888888888888888888888";
  const std::string badHash = "badddddddddddddddddddddddddddddddddddddd";

  handler->setGetBlobExpectation(hash, blob);

  auto result = store.getBlob(Hash(badHash))
                    .via(&folly::QueuedImmediateExecutor::instance())
                    .wait(kTimeout)
                    .result();
  EXPECT_TRUE(result.hasException());

  auto exception = result.exception().get_exception<MononokeAPIException>();
  EXPECT_EQ(exception->get_kind(), MononokeAPIExceptionKind::NotFound);
}

TEST_F(MononokeThriftTest, getTree) {
  const std::string treeHash = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
  const std::string firstHash = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
  const std::string secondHash = "cccccccccccccccccccccccccccccccccccccccc";
  const std::string contentSha1 = "dddddddddddddddddddddddddddddddddddddddd";
  const std::vector<MononokeFile> files = {
      makeFile("first", firstHash),
      makeFile("second", secondHash, contentSha1, 100),
  };

  handler->setGetTreeExpectation(treeHash, files);

  auto result = store.getTree(Hash(treeHash)).get(kTimeout);
  EXPECT_EQ(result->getHash(), Hash(treeHash));

  auto entries = result->getTreeEntries();
  auto first = entries.at(0);
  EXPECT_EQ(first.getName(), "first");
  EXPECT_EQ(first.getHash().toString(), firstHash);
  EXPECT_EQ(first.getContentSha1(), std::nullopt);
  EXPECT_EQ(first.getSize(), std::nullopt);

  auto second = entries.at(1);
  EXPECT_EQ(second.getName(), "second");
  EXPECT_EQ(second.getHash().toString(), secondHash);
  EXPECT_EQ(second.getContentSha1()->toString(), contentSha1);
  EXPECT_EQ(second.getSize(), 100);
}

TEST_F(MononokeThriftTest, getTreeNotFound) {
  const std::string treeHash = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
  const std::string badHash = "badddddddddddddddddddddddddddddddddddddd";
  const std::vector<MononokeFile> files = {};

  handler->setGetTreeExpectation(treeHash, files);

  auto result = store.getTree(Hash(badHash))
                    .via(&folly::QueuedImmediateExecutor::instance())
                    .wait(kTimeout)
                    .result();
  EXPECT_TRUE(result.hasException());

  auto exception = result.exception().get_exception<MononokeAPIException>();
  EXPECT_EQ(exception->get_kind(), MononokeAPIExceptionKind::NotFound);
}

TEST_F(MononokeThriftTest, getTreeForCommit) {
  const std::string blob = "hello";
  const std::string changesetHash = "8888888888888888888888888888888888888888";
  const std::string manifest = "ffffffffffffffffffffffffffffffffffffffff";
  const std::vector<MononokeFile> files = {
      makeFile("file", "ffffffffffffffffffffffffffffffffffffffff"),
  };

  handler->setGetTreeExpectation(manifest, files);
  handler->setGetChangesetExpectation(changesetHash, manifest);

  auto result = store.getTreeForCommit(Hash(changesetHash)).get(kTimeout);
  auto entries = result->getTreeEntries();

  auto first = entries.at(0);
  EXPECT_EQ(first.getName(), "file");
}

TEST_F(MononokeThriftTest, getTreeForManifest) {
  const std::string blob = "hello";
  const std::string changesetHash = "8888888888888888888888888888888888888888";
  const std::string manifest = "ffffffffffffffffffffffffffffffffffffffff";
  const std::vector<MononokeFile> files = {
      makeFile("file", "ffffffffffffffffffffffffffffffffffffffff"),
  };

  handler->setGetTreeExpectation(manifest, files);
  handler->setGetChangesetExpectation(changesetHash, manifest);

  auto result = store.getTreeForManifest(Hash(changesetHash), Hash(manifest))
                    .via(&folly::QueuedImmediateExecutor::instance())
                    .get(kTimeout);
  auto entries = result->getTreeEntries();

  auto first = entries.at(0);
  EXPECT_EQ(first.getName(), "file");
}

TEST_F(MononokeThriftTest, getChangesetNotFound) {
  const std::string changesetHash = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
  const std::string manifest = "ffffffffffffffffffffffffffffffffffffffff";
  const std::string badHash = "badddddddddddddddddddddddddddddddddddddd";

  handler->setGetChangesetExpectation(changesetHash, manifest);

  auto result = store.getTreeForCommit(Hash(badHash))
                    .via(&folly::QueuedImmediateExecutor::instance())
                    .wait(kTimeout)
                    .result();
  EXPECT_TRUE(result.hasException());

  auto exception = result.exception().get_exception<MononokeAPIException>();
  EXPECT_EQ(exception->get_kind(), MononokeAPIExceptionKind::NotFound);
}
