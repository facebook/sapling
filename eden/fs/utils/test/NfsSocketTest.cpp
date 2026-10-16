/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/NfsSocket.h"

#include <gtest/gtest.h>

#include <string>

using namespace facebook::eden;

namespace {

// An absolute directory path of exactly the given length.
AbsolutePath directoryOfLength(size_t length) {
  return canonicalPath("/" + std::string(length - 1, 'd'));
}

// The length of a checkout's nfsd socket path for a name of the given length.
size_t nfsdSocketPathLength(const AbsolutePath& clientsDir, size_t nameLength) {
  return clientsDir.view().size() + 1 + nameLength + 1 +
      kNfsdSocketName.view().size();
}

// The length of a checkout name whose socket path is exactly the limit.
size_t longestFittingName(const AbsolutePath& clientsDir) {
  return kMaxUnixSocketPathLength - nfsdSocketPathLength(clientsDir, 0);
}

} // namespace

TEST(NfsSocket, shortPathsFit) {
  auto edenDir = directoryOfLength(20);
  EXPECT_EQ(
      std::nullopt,
      whyNfsCannotUseUnixSockets(
          edenDir + "mountd.socket"_pc,
          edenDir + "clients"_pc,
          {"fbsource", "www", std::string(kReservedCheckoutNameLength, 'n')}));
}

TEST(NfsSocket, aCheckoutWhoseSocketPathDoesNotFitForcesTcp) {
  auto edenDir = directoryOfLength(20);
  auto clientsDir = edenDir + "clients"_pc;
  std::string fits(longestFittingName(clientsDir), 'n');
  std::string tooLong(fits.size() + 1, 'n');

  EXPECT_EQ(
      std::nullopt,
      whyNfsCannotUseUnixSockets(
          edenDir + "mountd.socket"_pc, clientsDir, {"fbsource", fits}));
  auto why = whyNfsCannotUseUnixSockets(
      edenDir + "mountd.socket"_pc, clientsDir, {"fbsource", tooLong});
  ASSERT_TRUE(why.has_value());
  EXPECT_NE(std::string::npos, why->find("the nfsd socket path"));
  EXPECT_NE(std::string::npos, why->find(tooLong));
}

TEST(NfsSocket, roomIsKeptForACheckoutOfOrdinaryNameLength) {
  // A state directory so deep that "fbsource" fits but a 16-byte name does
  // not: the daemon must not commit to Unix sockets.
  auto clientsDir = directoryOfLength(
      kMaxUnixSocketPathLength - 2 - kNfsdSocketName.view().size() -
      (kReservedCheckoutNameLength - 1));
  ASSERT_LE(nfsdSocketPathLength(clientsDir, 8), kMaxUnixSocketPathLength);
  ASSERT_GT(
      nfsdSocketPathLength(clientsDir, kReservedCheckoutNameLength),
      kMaxUnixSocketPathLength);

  auto why = whyNfsCannotUseUnixSockets(
      directoryOfLength(20) + "mountd.socket"_pc, clientsDir, {"fbsource"});
  ASSERT_TRUE(why.has_value());
  EXPECT_NE(std::string::npos, why->find("16-byte name"));
}

TEST(NfsSocket, aMountdSocketPathThatDoesNotFitForcesTcp) {
  auto mountdSocketPath =
      directoryOfLength(kMaxUnixSocketPathLength) + "mountd.socket"_pc;
  auto why = whyNfsCannotUseUnixSockets(
      mountdSocketPath, directoryOfLength(20) + "clients"_pc, {});
  ASSERT_TRUE(why.has_value());
  EXPECT_NE(std::string::npos, why->find("the mountd socket path"));
}
