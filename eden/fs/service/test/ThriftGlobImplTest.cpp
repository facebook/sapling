/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/portability/GTest.h>
#include <cstddef>
#include <memory>

#include "eden/fs/inodes/InodeMap.h"
#include "eden/fs/service/ThriftGlobImpl.h"
#include "eden/fs/service/gen-cpp2/eden_types.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/testharness/FakeTreeBuilder.h"
#include "eden/fs/testharness/TestMount.h"
#include "eden/fs/testharness/TestServerState.h"

namespace facebook::eden {

std::tuple<size_t, size_t> getInodeCounters(InodeMap* map) {
  auto counter = map->getInodeCounts();
  auto loaded = counter.fileCount + counter.treeCount;
  auto unloaded = counter.unloadedInodeCount;
  return std::make_tuple(loaded, unloaded);
}

void assertInodeCounters(
    InodeMap* map,
    size_t expectedLoaded,
    size_t expectedUnloaded) {
  auto [loaded, unloaded] = getInodeCounters(map);
  ASSERT_EQ(loaded, expectedLoaded);
  ASSERT_EQ(unloaded, expectedUnloaded);
}

TEST(ThriftGlobImplTest, testGlobFilesNotLoadingInode) {
  auto serverState = createTestServerState();
  FakeTreeBuilder builder;
  builder.setFile("foo/bar/dir1/file.txt", "contents");
  builder.setFile("foo/bar/dir2/file.txt", "contents");
  TestMount mount{builder};
  auto edenMount = mount.getEdenMount();
  auto* inodeMap = edenMount->getInodeMap();

  // We get the loaded number before the thrift call. We always load root tree
  // after initialize.
  auto [loaded, unloaded] = getInodeCounters(inodeMap);

  std::string glob{"**/*.txt"};
  auto globber = ThriftGlobImpl{GlobParams{}};
  auto globFuture = globber.glob(
      edenMount,
      serverState,
      std::vector<std::string>{"**/*.txt"},
      ObjectFetchContext::getNullContext());

  auto _result = std::move(globFuture).get();

  // Then we compare the number, both counter should remain the same before and
  // after the call.
  assertInodeCounters(inodeMap, loaded, unloaded);

  // Then we read these two files, making sure they are loaded
  auto content1 = mount.readFile("foo/bar/dir1/file.txt");
  auto content2 = mount.readFile("foo/bar/dir2/file.txt");

  // We should observe the loaded counter to be up by 6. Inodes loaded here are:
  // - foo
  // - foo/bar
  // - foo/bar/dir1
  // - foo/bar/dir1/file.txt
  // - foo/bar/dir2
  // - foo/bar/dir2/file.txt
  assertInodeCounters(inodeMap, loaded + 6, unloaded);
}
} // namespace facebook::eden
