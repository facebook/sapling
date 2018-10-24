/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/utils/ProcUtil.h"

#include <eden/fs/utils/PathFuncs.h>
#include <folly/ExceptionWrapper.h>
#include <gtest/gtest.h>
#include <fstream>

using namespace facebook::eden;

TEST(proc_util, trimTest) {
  std::string tst("");
  EXPECT_EQ(proc_util::trim(tst), "");

  tst = "   spaceBefore";
  EXPECT_EQ(proc_util::trim(tst), "spaceBefore");

  tst = "spaceAfter   ";
  EXPECT_EQ(proc_util::trim(tst), "spaceAfter");

  tst = " spaceBeforeAfter ";
  EXPECT_EQ(proc_util::trim(tst), "spaceBeforeAfter");

  tst = " space between ";
  EXPECT_EQ(proc_util::trim(tst), "space between");

  tst = "noSpaces";
  EXPECT_EQ(proc_util::trim(tst), "noSpaces");

  tst = " \t\n\v\f\r";
  EXPECT_EQ(proc_util::trim(tst), "");

  tst = " \t\n\v\f\rtheGoods \t\n\v\f\r";
  EXPECT_EQ(proc_util::trim(tst), "theGoods");

  tst = "start \t\n\v\f\rend";
  EXPECT_EQ(proc_util::trim(tst), "start \t\n\v\f\rend");
}

TEST(proc_util, splitTest) {
  std::string line;

  line = "key : value";
  auto kvPair = proc_util::getKeyValuePair(line, ":");
  EXPECT_EQ(kvPair.first, "key");
  EXPECT_EQ(kvPair.second, "value");

  line = "    key :  value      ";
  kvPair = proc_util::getKeyValuePair(line, ":");
  EXPECT_EQ(kvPair.first, "key");
  EXPECT_EQ(kvPair.second, "value");

  line = "extra:colon:";
  kvPair = proc_util::getKeyValuePair(line, ":");
  EXPECT_EQ(kvPair.first, "");
  EXPECT_EQ(kvPair.second, "");

  line = "noColonHere";
  kvPair = proc_util::getKeyValuePair(line, ":");
  EXPECT_EQ(kvPair.first, "");
  EXPECT_EQ(kvPair.second, "");

  line = ":value";
  kvPair = proc_util::getKeyValuePair(line, ":");
  EXPECT_EQ(kvPair.first, "");
  EXPECT_EQ(kvPair.second, "value");

  line = ":";
  kvPair = proc_util::getKeyValuePair(line, ":");
  EXPECT_EQ(kvPair.first, "");
  EXPECT_EQ(kvPair.second, "");
}

TEST(proc_util, procStatusRssBytes) {
  auto procPath = realpath("eden/fs/utils/test/test-data/ProcStatus.txt");
  std::ifstream input(procPath.c_str());
  auto statMap = proc_util::parseProcStatus(input);
  auto rssBytes = proc_util::getUnsignedLongLongValue(
      statMap,
      std::string(kVmRSSKey.data(), kVmRSSKey.size()),
      std::string(kKBytes.data(), kKBytes.size()));
  EXPECT_EQ(statMap["VmRSS"], "1449644 kB");
  EXPECT_EQ(rssBytes.value(), 1449644);
}

TEST(proc_util, procStatusSomeInvalidInput) {
  auto procPath = realpath("eden/fs/utils/test/test-data/ProcStatusError.txt");
  std::ifstream input(procPath.c_str());
  auto statMap = proc_util::parseProcStatus(input);
  EXPECT_EQ(statMap["Name"], "edenfs");
  EXPECT_EQ(statMap["Umask"], "0022");
  EXPECT_EQ(statMap["VmRSS"], "1449644");
  EXPECT_EQ(statMap["Uid"], "131926\t131926\t131926\t131926");
  EXPECT_EQ(statMap["Gid"], "100\t100\t100\t100");
  EXPECT_EQ(statMap.size(), 5);

  auto rssBytes = proc_util::getUnsignedLongLongValue(
      statMap,
      std::string(kVmRSSKey.data(), kVmRSSKey.size()),
      std::string(kKBytes.data(), kKBytes.size()));

  EXPECT_EQ(rssBytes, std::nullopt);
}

TEST(proc_util, procStatusNoThrow) {
  std::string procPath("/DOES_NOT_EXIST");
  auto statMap = proc_util::loadProcStatus(procPath);
  auto rssBytes = proc_util::getUnsignedLongLongValue(
      statMap,
      std::string(kVmRSSKey.data(), kVmRSSKey.size()),
      std::string(kKBytes.data(), kKBytes.size()));
  EXPECT_EQ(rssBytes, std::nullopt);
}

TEST(proc_util, procSmapsPrivateBytes) {
  auto procPath = realpath("eden/fs/utils/test/test-data/ProcSmapsSimple.txt");
  std::ifstream input(procPath.c_str());
  auto smapsListOfMaps = proc_util::parseProcSmaps(input);
  auto privateBytes = proc_util::calculatePrivateBytes(smapsListOfMaps).value();
  EXPECT_EQ(privateBytes, 20 * 1024);
}

TEST(proc_util, procSmapsSomeInvalidInput) {
  auto procPath = realpath("eden/fs/utils/test/test-data/ProcSmapsError.txt");
  auto smapsListOfMaps = proc_util::loadProcSmaps(procPath.c_str());
  auto privateBytes = proc_util::calculatePrivateBytes(smapsListOfMaps).value();
  EXPECT_EQ(privateBytes, 4096);
}

TEST(proc_util, procSmapsUnknownFormat) {
  auto procPath =
      realpath("eden/fs/utils/test/test-data/ProcSmapsUnknownFormat.txt");
  auto smapsListOfMaps = proc_util::loadProcSmaps(procPath.c_str());
  auto privateBytes = proc_util::calculatePrivateBytes(smapsListOfMaps);
  EXPECT_EQ(privateBytes, std::nullopt);
}

TEST(proc_util, noProcSmapsNoThrow) {
  std::string procPath("/DOES_NOT_EXIST");
  auto smapsListOfMaps = proc_util::loadProcSmaps(procPath);
  auto privateBytes = proc_util::calculatePrivateBytes(smapsListOfMaps).value();
  EXPECT_EQ(privateBytes, 0);
}
