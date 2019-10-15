/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/FileUtil.h>
#include <folly/experimental/TestUtil.h>
#include <folly/logging/xlog.h>
#include <gmock/gmock.h>
#include <gtest/gtest.h>
#include <memory>

#include "eden/fs/model/Tree.h"
#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/store/MemoryLocalStore.h"
#include "eden/fs/store/hg/HgBackingStore.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/testharness/HgRepo.h"
#include "eden/fs/utils/PathFuncs.h"
#include "eden/fs/utils/UnboundedQueueExecutor.h"

using namespace facebook::eden;
using namespace std::chrono_literals;
using folly::StringPiece;
using folly::test::TemporaryDirectory;
using testing::HasSubstr;

TEST(HgPrefetch, test) {
  MemoryLocalStore localStore;
  auto stats = std::make_shared<EdenStats>();
  TemporaryDirectory testDir("eden_hg_import_test");
  AbsolutePath testPath{testDir.path().string()};

  // Write a dummy ssh wrapper script
  AbsolutePath dummySshPath = testPath + "dummyssh"_pc;
  std::string dummySsh = R"(
#!/bin/bash

if [[ $# -ne 2 ]]; then
  echo "unexpected number of ssh arguments: $@" >&2
  exit 1
fi
if [[ $1 != "user@dummy" ]]; then
  echo "unexpected ssh user argument: $@" >&2
  exit 1
fi
if ! [[ $2 =~ "hg " ]]; then
  echo "unexpected ssh command argument: $@" >&2
  exit 1
fi

exec $2
)";
  ASSERT_TRUE(folly::writeFile(
      dummySsh,
      dummySshPath.value().c_str(),
      O_WRONLY | O_CREAT | O_TRUNC,
      0755));

  AbsolutePath systemHgrcPath = testPath + "hgrc"_pc;
  auto baseHgrc = folly::sformat(
      R"(
[ui]
ssh = {0}

[extensions]
fastmanifest =
treemanifest =
remotefilelog =

[remotefilelog]
pullprefetch =
bgprefetchrevs =
backgroundrepack = False
backgroundprefetch = False
reponame = eden_test_hg_prefetch

[fastmanifest]
usetree=True
cacheonchange=True
usecache=False

[treemanifest]
usecunionstore=True
)",
      dummySshPath.value());
  ASSERT_TRUE(folly::writeFile(baseHgrc, systemHgrcPath.value().c_str()));

  // Create the server-side repository
  HgRepo serverRepo{testPath + "server_repo"_pc};
  serverRepo.hgInit({"--configfile", systemHgrcPath.value()});
  serverRepo.appendToHgrc(baseHgrc);
  serverRepo.appendToHgrc({
      "[remotefilelog]",
      "server = True",
      "cachepath = " + (testPath + "server_hgcache"_pc).value(),
      ""
      "[treemanifest]",
      "server = True",
      "",
  });

  // Create some test commits in the server repository
  serverRepo.mkdir("foo");
  StringPiece barData = "this is a test file\n";
  serverRepo.writeFile("foo/bar.txt", barData);
  StringPiece testData = "testing\n1234\ntesting\n";
  serverRepo.writeFile("foo/test.txt", testData);
  serverRepo.mkdir("src");
  serverRepo.mkdir("src/eden");
  StringPiece somelinkData = "this is the link contents";
  serverRepo.symlink(somelinkData, "src/somelink"_relpath);
  StringPiece mainData = "print('hello world\\n')\n";
  serverRepo.writeFile("src/eden/main.py", mainData, 0755);
  serverRepo.hg("add");
  serverRepo.commit("Initial commit");

  StringPiece mainData2 = "print('hello brave new world\\n')\n";
  serverRepo.writeFile("src/eden/main.py", mainData2, 0755);
  StringPiece abcData = "aaa\nbbb\nccc\n";
  serverRepo.writeFile("src/eden/abc.py", abcData, 0644);
  // Include a file with non-ASCII data in the file name.
  // Mercurial wants file names to be valid UTF-8.
  auto binaryFileName =
      "\xc5\xa4"
      "\xc3\xaa"
      "\xc5\x9b"
      "\xc5\xa5.dat"_pc;
  serverRepo.writeFile(
      "src/eden/" + binaryFileName.value().str(), "data", 0755);
  serverRepo.hg("add");
  auto commit2 = serverRepo.commit("Commit 2");

  serverRepo.writeFile("src/eden/main.py", "blah", 0755);
  serverRepo.commit("Commit 3");

  // Create the client-side repository
  HgRepo clientRepo{testPath + "client_repo"_pc};
  clientRepo.cloneFrom(
      folly::to<std::string>("ssh://user@dummy/", serverRepo.path()),
      {"--shallow",
       "--configfile",
       systemHgrcPath.value(),
       "--config",
       "remotefilelog.cachepath=" + (testPath + "client_hgcache"_pc).value()});
  clientRepo.appendToHgrc(baseHgrc);
  clientRepo.appendToHgrc({
      "[remotefilelog]",
      "cachepath = " + (testPath + "client_hgcache"_pc).value(),
      "",
  });

  // Running "hg cat" with no server repo should fail before we run prefetch
  auto catProcess = clientRepo.invokeHg(
      {"--config",
       "paths.default=",
       "--config",
       "ui.ssh=/bin/false",
       "cat",
       "-r",
       commit2.toString(),
       "src/eden/main.py"},
      folly::Subprocess::Options()
          .chdir(clientRepo.path().value())
          .pipeStdout()
          .pipeStderr());
  auto catOutputs{catProcess.communicate()};
  auto returnCode = catProcess.wait();
  EXPECT_EQ(folly::ProcessReturnCode::EXITED, returnCode.state());
  EXPECT_NE(0, returnCode.exitStatus());
  EXPECT_THAT(
      catOutputs.second, HasSubstr("no remotefilelog server configured"));

  // Build an HgBackingStore for this repository
  UnboundedQueueExecutor resultThreadPool(1, "ResultThread");
  HgBackingStore store(
      clientRepo.path(), &localStore, &resultThreadPool, nullptr, stats);

  // Now test running prefetch
  // Build a list of file blob IDs to prefetch.
  auto rootTree = store.getTreeForCommit(commit2).get(10s);
  auto srcTree =
      store.getTree(rootTree->getEntryAt("src"_pc).getHash()).get(10s);
  auto edenTree =
      store.getTree(srcTree->getEntryAt("eden"_pc).getHash()).get(10s);
  auto fooTree =
      store.getTree(rootTree->getEntryAt("foo"_pc).getHash()).get(10s);

  std::vector<Hash> blobHashes;
  blobHashes.push_back(edenTree->getEntryAt("main.py"_pc).getHash());
  blobHashes.push_back(edenTree->getEntryAt("abc.py"_pc).getHash());
  blobHashes.push_back(edenTree->getEntryAt("abc.py"_pc).getHash());
  blobHashes.push_back(edenTree->getEntryAt(binaryFileName).getHash());
  blobHashes.push_back(srcTree->getEntryAt("somelink"_pc).getHash());
  blobHashes.push_back(fooTree->getEntryAt("bar.txt"_pc).getHash());
  blobHashes.push_back(fooTree->getEntryAt("test.txt"_pc).getHash());

  // Call prefetchBlobs()
  store.prefetchBlobs(blobHashes).get(10s);

  // Running "hg cat" with ssh disabled and no server repo should succeed now
  // that we have prefetched the data.
  //
  // The treemanifest extension code currently seems to always connect to the
  // server even if it doesn't need to download any data.  Setting paths.default
  // to the empty string works around this behavior.
  clientRepo.hg(
      "--config",
      "paths.default=",
      "--config",
      "ui.ssh=/bin/false",
      "--traceback",
      "cat",
      "-r",
      commit2.toString(),
      "src/eden/main.py");
}
