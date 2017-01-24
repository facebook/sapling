/*
 *  Copyright (c) 2017-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include <folly/FileUtil.h>
#include <folly/experimental/TestUtil.h>
#include <gtest/gtest.h>
#include "eden/fs/config/InterpolatedPropertyTree.h"

namespace {
using folly::test::TemporaryDirectory;
using namespace facebook::eden;

class InterpTest : public ::testing::Test {
 protected:
  std::unique_ptr<TemporaryDirectory> tmpDir_;
  virtual void SetUp() override {
    tmpDir_ = std::make_unique<TemporaryDirectory>("eden_interp_test_");
  }
  virtual void TearDown() override {
    tmpDir_.reset();
  }
};

TEST_F(InterpTest, testFunctionality) {
  folly::StringPiece data(
      "[section]\n"
      "name = value\n"
      "path = ${HOME}\n"
      "sub = foo${HOME}bar${HOME}baz\n"
      "recursive = a${RECURSE}b\n");
  auto iniName = AbsolutePath((tmpDir_->path() / "foo.ini").c_str());
  folly::writeFile(data, iniName.c_str());

  InterpolatedPropertyTree tree;
  tree.loadIniFile(iniName);

  EXPECT_FALSE(tree.hasSection("invalid"));
  EXPECT_TRUE(tree.hasSection("section"));
  EXPECT_EQ("nope", tree.get("invalid", "foo", "nope"))
      << "Missing section uses default value";
  EXPECT_EQ("value", tree.get("section", "name", "nope"))
      << "returns the value for the requested section and key";
  EXPECT_EQ("nope", tree.get("section", "missing", "nope"))
      << "missing key in a found section uses default value";
  EXPECT_EQ("${HOME}", tree.get("section", "path", "nope"))
      << "no interpolation happens when no replacements have been provided";

  InterpolatedPropertyTree interpTree{{"HOME", "/home/wez"},
                                      {"RECURSE", "foo${RECURSE}"}};
  interpTree.loadIniFile(AbsolutePathPiece{iniName.c_str()});
  EXPECT_EQ("value", interpTree.get("section", "name", "nope"));
  EXPECT_EQ("nope", interpTree.get("section", "missing", "nope"));
  EXPECT_EQ("/home/wez", interpTree.get("section", "path", "nope"))
      << "basic interpolation succeeded";
  EXPECT_EQ(
      "foo/home/wezbar/home/wezbaz", interpTree.get("section", "sub", "nope"))
      << "interpolated the HOME variable multiple times";
  EXPECT_EQ("afoo${RECURSE}b", interpTree.get("section", "recursive", ""))
      << "self referential value fetch halts deterministically";
}

TEST_F(InterpTest, testReferenceCycle) {
  folly::StringPiece data(
      "[section]\n"
      "foo = ${USER}\n");

  auto iniName = AbsolutePath((tmpDir_->path() / "foo.ini").c_str());
  folly::writeFile(data, iniName.c_str());

  InterpolatedPropertyTree tree{{"USER", "${HOME}"}, {"HOME", "foo"}};
  tree.loadIniFile(iniName);

  EXPECT_EQ("${HOME}", tree.get("section", "foo", "nope"));
}

TEST_F(InterpTest, testSet) {
  InterpolatedPropertyTree tree;

  tree.set("foo", "bar", "baz");
  EXPECT_EQ("baz", tree.get("foo", "bar", "nope"));

  tree.set("foo", "wat", "woot");
  EXPECT_EQ("woot", tree.get("foo", "wat", "nope"));

  tree.set("other", "key", "value");
  EXPECT_EQ("value", tree.get("other", "key", "nope"));
}

TEST_F(InterpTest, testMerge) {
  folly::StringPiece base(
      "[section]\n"
      "name = value\n");
  folly::StringPiece repo1(
      "[repo one]\n"
      "name = one\n");
  folly::StringPiece repo2(
      "[repo one]\n"
      "name = replacedname\n"
      "extra = arg\n"
      "[repo two]\n"
      "name = two\n");

  auto baseName = AbsolutePath((tmpDir_->path() / "base.ini").c_str());
  folly::writeFile(base, baseName.c_str());
  auto oneName = AbsolutePath((tmpDir_->path() / "one.ini").c_str());
  folly::writeFile(repo1, oneName.c_str());
  auto twoName = AbsolutePath((tmpDir_->path() / "two.ini").c_str());
  folly::writeFile(repo2, twoName.c_str());

  InterpolatedPropertyTree tree;
  tree.updateFromIniFile(baseName);

  EXPECT_TRUE(tree.hasSection("section"));
  EXPECT_EQ("value", tree.get("section", "name", "nope"));

  // A function that prevents merging a repo stanza over a pre-existing one
  auto accept = [](
      const InterpolatedPropertyTree& tree, folly::StringPiece section) {
    if (section.startsWith("repo ") && tree.hasSection(section)) {
      return InterpolatedPropertyTree::MergeDisposition::SkipAll;
    }
    return InterpolatedPropertyTree::MergeDisposition::UpdateAll;
  };

  tree.updateFromIniFile(oneName, accept);
  EXPECT_TRUE(tree.hasSection("repo one"))
      << "allowed repo one because it wasn't already there";
  EXPECT_TRUE(tree.hasSection("section"))
      << "didn't replace the existing section";
  EXPECT_EQ("one", tree.get("repo one", "name", "nope"));

  tree.updateFromIniFile(twoName, accept);
  EXPECT_TRUE(tree.hasSection("repo one"));
  EXPECT_TRUE(tree.hasSection("repo two"));

  EXPECT_EQ("one", tree.get("repo one", "name", "nope"))
      << "name didn't get replaced with the name from repo2";
  EXPECT_EQ("nope", tree.get("repo one", "extra", "nope"))
      << "didn't merge in the 'extra' entry from repo2";

  EXPECT_EQ("two", tree.get("repo two", "name", "nope"));

  // Can't use EXPECT_EQ on these because gtest wants to do something weird
  // with the private base class of StringKeyedUnorderedMap
  EXPECT_TRUE(
      folly::StringKeyedUnorderedMap<std::string>({{"name", "one"}}) ==
      tree.getSection("repo one"));
  EXPECT_TRUE(
      folly::StringKeyedUnorderedMap<std::string>({{"name", "two"}}) ==
      tree.getSection("repo two"));
  EXPECT_TRUE(
      folly::StringKeyedUnorderedMap<std::string>({{"name", "value"}}) ==
      tree.getSection("section"));

  // and check that the default UpdateAll policy for updateFromIniFile
  // works as expected
  tree.updateFromIniFile(twoName);
  EXPECT_TRUE(
      folly::StringKeyedUnorderedMap<std::string>(
          {{"name", "replacedname"}, {"extra", "arg"}}) ==
      tree.getSection("repo one"));
  EXPECT_TRUE(
      folly::StringKeyedUnorderedMap<std::string>({{"name", "two"}}) ==
      tree.getSection("repo two"));
}
}
