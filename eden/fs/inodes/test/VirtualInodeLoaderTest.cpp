/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/VirtualInodeLoader.h"
#include <folly/coro/GtestHelpers.h>
#include <folly/test/TestUtils.h>
#include <folly/testing/TestUtil.h>
#include <gtest/gtest.h>
#include <optional>
#include <string_view>
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/testharness/FakeBackingStore.h"
#include "eden/fs/testharness/FakeTreeBuilder.h"
#include "eden/fs/testharness/TestChecks.h"
#include "eden/fs/testharness/TestMount.h"

using namespace facebook::eden;
using namespace std::literals::chrono_literals;

// VirtualInode objects don't currently know or can compute their paths,
// as once you switch from the Inode objects to => DirEntry/Tree/TreeEntry, you
// lose track of the parent object (unlike inodes, which always know their
// parent). Rather than keep paths around just to report them for this test,
// instead we set the file contents to be their own absolute paths, so we can
// compare the hashes instead.
namespace {
#define FILES {{"dir/a.txt", "dir/a.txt"}, {"dir/sub/b.txt", "dir/sub/b.txt"}}
// SHA-1 of "dir/a.txt".
constexpr Hash20 kDirATxtSha1{
    folly::StringPiece{"cb1fe72e0440dcd1dbe205965c7b48659e8c9bbb"}};
// SHA-1 of "dir/sub/b.txt".
constexpr Hash20 kDirSubBTxtSha1{
    folly::StringPiece{"cbf507d4a3137f6bbf6ecb72da9f3b79b7178e2f"}};

template <typename T>
void expectErrno(const folly::Try<T>& result, int expectedErrno) {
  ASSERT_TRUE(result.hasException());
  const auto* error =
      result.exception().template get_exception<std::system_error>();
  ASSERT_NE(nullptr, error);
  EXPECT_EQ(expectedErrno, error->code().value());
}

template <typename T>
void expectDomainError(
    const folly::Try<T>& result,
    std::string_view expectedMessage) {
  ASSERT_TRUE(result.hasException());
  const auto* error =
      result.exception().template get_exception<std::domain_error>();
  ASSERT_NE(nullptr, error);
  EXPECT_NE(
      std::string_view{error->what()}.find(expectedMessage),
      std::string_view::npos);
}
} // namespace

CO_TEST(CoInodeLoader, loadSHA1) {
  FakeTreeBuilder builder;
  builder.setFiles(FILES);
  TestMount mount(builder);

  auto rootInode = mount.getTreeInode(RelativePathPiece());
  auto objectStore = mount.getEdenMount()->getObjectStore();
  auto fetchContext = ObjectFetchContext::getNullContext();

  {
    auto results = co_await co_applyToVirtualInode(
        rootInode,
        std::vector<std::string>{
            "dir/a.txt", "not/exist/a", "not/exist/b", "dir/sub/b.txt"},
        [objectStore, fetchContext = fetchContext.copy()](
            VirtualInode inode,
            RelativePath path) -> folly::coro::now_task<Hash20> {
          co_return co_await inode.co_getSHA1(path, objectStore, fetchContext);
        },
        objectStore,
        fetchContext);

    EXPECT_EQ(kDirATxtSha1, results[0].value());
    expectErrno(results[1], ENOENT);
    expectErrno(results[2], ENOENT);
    EXPECT_EQ(kDirSubBTxtSha1, results[3].value());
  }

  {
    auto results = co_await co_applyToVirtualInode(
        rootInode,
        std::vector<std::string>{
            "dir/sub/b.txt",
            "dir/a.txt",
            "not/exist/a",
            "not/exist/b",
            "dir/sub/b.txt"},
        [objectStore, fetchContext = fetchContext.copy()](
            VirtualInode inode,
            RelativePath path) -> folly::coro::now_task<Hash20> {
          co_return co_await inode.co_getSHA1(path, objectStore, fetchContext);
        },
        objectStore,
        fetchContext);

    EXPECT_EQ(kDirSubBTxtSha1, results[0].value());
    EXPECT_EQ(kDirATxtSha1, results[1].value());
    expectErrno(results[2], ENOENT);
    expectErrno(results[3], ENOENT);
    EXPECT_EQ(results[0].value(), results[4].value())
        << "dir/sub/b.txt was requested twice and both entries are the same";
  }

  {
    auto results = co_await co_applyToVirtualInode(
        rootInode,
        std::vector<std::string>{"dir/a.txt", "/invalid///exist/a"},
        [objectStore, fetchContext = fetchContext.copy()](
            VirtualInode inode,
            RelativePath path) -> folly::coro::now_task<Hash20> {
          co_return co_await inode.co_getSHA1(path, objectStore, fetchContext);
        },
        objectStore,
        fetchContext);

    EXPECT_EQ(kDirATxtSha1, results[0].value());
    expectDomainError(results[1], "absolute path");
  }
}

TEST(InodeLoader, notReady) {
  FakeTreeBuilder builder;
  builder.setFiles(FILES);
  TestMount mount(builder, /* startReady= */ false);

  auto rootInode = mount.getTreeInode(RelativePathPiece());
  auto objectStore = mount.getEdenMount()->getObjectStore();
  auto fetchContext = ObjectFetchContext::getNullContext();

  {
    // @lint-ignore CLANGTIDY facebook-folly-coro-return-captures-local-var
    auto future =
        folly::coro::co_invoke(
            [&]() -> folly::coro::Task<std::vector<folly::Try<Hash20>>> {
              co_return co_await co_applyToVirtualInode(
                  rootInode,
                  std::vector<std::string>{
                      "dir/a.txt",
                      "not/exist/a",
                      "not/exist/b",
                      "dir/sub/b.txt"},
                  [objectStore, fetchContext = fetchContext.copy()](
                      VirtualInode inode,
                      RelativePath path) -> folly::coro::now_task<Hash20> {
                    co_return co_await inode.co_getSHA1(
                        path, objectStore, fetchContext);
                  },
                  objectStore,
                  fetchContext);
            })
            .semi()
            .via(mount.getServerExecutor().get());

    mount.drainServerExecutor();
    EXPECT_FALSE(future.isReady());

    builder.setReady("dir");
    builder.setReady("dir/sub");
    builder.setReady("dir/a.txt");
    builder.setReady("dir/sub/b.txt");

    mount.drainServerExecutor();
    EXPECT_TRUE(future.isReady());
    auto results = std::move(future).get(0ms);

    EXPECT_EQ(kDirATxtSha1, results[0].value());
    EXPECT_THROW_ERRNO(results[1].value(), ENOENT);
    EXPECT_THROW_ERRNO(results[2].value(), ENOENT);
    EXPECT_EQ(kDirSubBTxtSha1, results[3].value());
  }
}

CO_TEST(CoInodeLoader, loadBlake3) {
  FakeTreeBuilder builder;
  builder.setFiles(FILES);
  TestMount mount(builder);

  auto rootInode = mount.getTreeInode(RelativePathPiece());
  auto objectStore = mount.getEdenMount()->getObjectStore();
  auto fetchContext = ObjectFetchContext::getNullContext();

  {
    // Exercise co_applyToVirtualInode with a now_task-returning func
    // (co_getBlake3), covering the CoResultOf<now_task<T>> trait path.
    auto results = co_await co_applyToVirtualInode(
        rootInode,
        std::vector<std::string>{
            "dir/a.txt", "not/exist/a", "not/exist/b", "dir/sub/b.txt"},
        [objectStore, fetchContext = fetchContext.copy()](
            VirtualInode inode,
            RelativePath path) -> folly::coro::now_task<Hash32> {
          co_return co_await inode.co_getBlake3(
              path, objectStore, fetchContext);
        },
        objectStore,
        fetchContext);

    EXPECT_EQ(
        Hash32::blake3(folly::ByteRange{folly::StringPiece{"dir/a.txt"}}),
        results[0].value());
    expectErrno(results[1], ENOENT);
    expectErrno(results[2], ENOENT);
    EXPECT_EQ(
        Hash32::blake3(folly::ByteRange{folly::StringPiece{"dir/sub/b.txt"}}),
        results[3].value());
  }

  {
    // Verify duplicate paths return the same blake3 hash.
    auto results = co_await co_applyToVirtualInode(
        rootInode,
        std::vector<std::string>{"dir/a.txt", "dir/sub/b.txt", "dir/a.txt"},
        [objectStore, fetchContext = fetchContext.copy()](
            VirtualInode inode,
            RelativePath path) -> folly::coro::now_task<Hash32> {
          co_return co_await inode.co_getBlake3(
              path, objectStore, fetchContext);
        },
        objectStore,
        fetchContext);

    EXPECT_EQ(results[0].value(), results[2].value())
        << "dir/a.txt was requested twice and both entries are the same";
  }

  {
    // Verify malformed paths surface as per-result exceptions.
    auto results = co_await co_applyToVirtualInode(
        rootInode,
        std::vector<std::string>{"dir/a.txt", "/invalid///exist/a"},
        [objectStore, fetchContext = fetchContext.copy()](
            VirtualInode inode,
            RelativePath path) -> folly::coro::now_task<Hash32> {
          co_return co_await inode.co_getBlake3(
              path, objectStore, fetchContext);
        },
        objectStore,
        fetchContext);

    EXPECT_EQ(
        Hash32::blake3(folly::ByteRange{folly::StringPiece{"dir/a.txt"}}),
        results[0].value());
    expectDomainError(results[1], "absolute path");
  }
}

CO_TEST(CoInodeLoader, loadDigestHash) {
  FakeTreeBuilder builder;
  builder.setFiles(FILES);
  TestMount mount(builder);

  auto rootInode = mount.getTreeInode(RelativePathPiece());
  auto objectStore = mount.getEdenMount()->getObjectStore();
  auto fetchContext = ObjectFetchContext::getNullContext();

  {
    // Exercise co_applyToVirtualInode with a now_task returning an optional,
    // matching the co_getDigestHash result type.
    auto results = co_await co_applyToVirtualInode(
        rootInode,
        std::vector<std::string>{
            "dir/a.txt", "not/exist/a", "not/exist/b", "dir/sub/b.txt"},
        [objectStore, fetchContext = fetchContext.copy()](
            VirtualInode inode,
            RelativePath path) -> folly::coro::now_task<std::optional<Hash32>> {
          co_return co_await inode.co_getDigestHash(
              path, objectStore, fetchContext);
        },
        objectStore,
        fetchContext);

    EXPECT_EQ(
        std::optional<Hash32>{
            Hash32::blake3(folly::ByteRange{folly::StringPiece{"dir/a.txt"}})},
        results[0].value());
    expectErrno(results[1], ENOENT);
    expectErrno(results[2], ENOENT);
    EXPECT_EQ(
        std::optional<Hash32>{Hash32::blake3(
            folly::ByteRange{folly::StringPiece{"dir/sub/b.txt"}})},
        results[3].value());
  }

  {
    // Verify duplicate paths return the same digest hash.
    auto results = co_await co_applyToVirtualInode(
        rootInode,
        std::vector<std::string>{"dir/a.txt", "dir/sub/b.txt", "dir/a.txt"},
        [objectStore, fetchContext = fetchContext.copy()](
            VirtualInode inode,
            RelativePath path) -> folly::coro::now_task<std::optional<Hash32>> {
          co_return co_await inode.co_getDigestHash(
              path, objectStore, fetchContext);
        },
        objectStore,
        fetchContext);

    EXPECT_EQ(results[0].value(), results[2].value())
        << "dir/a.txt was requested twice and both entries are the same";
  }

  {
    // Verify malformed paths surface as per-result exceptions.
    auto results = co_await co_applyToVirtualInode(
        rootInode,
        std::vector<std::string>{"dir/a.txt", "/invalid///exist/a"},
        [objectStore, fetchContext = fetchContext.copy()](
            VirtualInode inode,
            RelativePath path) -> folly::coro::now_task<std::optional<Hash32>> {
          co_return co_await inode.co_getDigestHash(
              path, objectStore, fetchContext);
        },
        objectStore,
        fetchContext);

    EXPECT_EQ(
        std::optional<Hash32>{
            Hash32::blake3(folly::ByteRange{folly::StringPiece{"dir/a.txt"}})},
        results[0].value());
    expectDomainError(results[1], "absolute path");
  }
}
