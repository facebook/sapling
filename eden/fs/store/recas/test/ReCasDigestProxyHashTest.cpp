/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/recas/ReCasDigestProxyHash.h"

#include <folly/Memory.h>
#include <folly/Range.h>
#include <gtest/gtest.h>
#include <stdexcept>

#include "eden/fs/model/Hash.h"
#include "eden/fs/store/MemoryLocalStore.h"
#include "remote_execution/lib/if/gen-cpp2/common_types.h"

using namespace facebook::eden;

struct ReCasDigestProxyHashTest : public ::testing::Test {
  ReCasDigestProxyHashTest() {}
  facebook::remote_execution::TDigest makeDigest(
      std::string hash,
      uint64_t size) {
    facebook::remote_execution::TDigest digest;
    digest.hash_ref() = std::move(hash);
    digest.set_size_in_bytes(size);
    return digest;
  }
};

TEST_F(ReCasDigestProxyHashTest, testSaveAndLoad) {
  const std::string hashString = "DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD";
  const uint64_t size = 20;
  auto store = std::make_shared<MemoryLocalStore>();
  Hash hash1;
  auto write = store->beginWrite();
  hash1 =
      ReCasDigestProxyHash::store(makeDigest(hashString, size), write.get());
  write->flush();

  auto digest = ReCasDigestProxyHash::load(store.get(), hash1, "test");
  EXPECT_TRUE(digest.has_value());
  auto digestValue = digest.value().digest();
  EXPECT_EQ(digestValue.get_hash(), hashString);
  EXPECT_EQ(digestValue.get_size_in_bytes(), size);
  EXPECT_EQ(digestValue, makeDigest(hashString, size));
}

TEST_F(ReCasDigestProxyHashTest, testSerialization) {
  const std::string hashString = "DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD";
  const uint64_t size = 20;
  const std::string serializedString =
      "DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD:20";
  EXPECT_EQ(
      ReCasDigestProxyHash::serialize(makeDigest(hashString, size)),
      serializedString);

  facebook::remote_execution::TDigest digest =
      ReCasDigestProxyHash::deserialize(serializedString);
  EXPECT_EQ(digest.get_hash(), hashString);
  EXPECT_EQ(digest.get_size_in_bytes(), size);
}

TEST_F(ReCasDigestProxyHashTest, testBadSerializationAndDeserialization) {
  const std::string badSerializedString =
      "DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD20";
  const std::string badSerializedString2 =
      "DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD:10";
  const std::string badHashString = "DDD";
  const std::string badHashString2 =
      "DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD20";
  const uint64_t size = 20;

  EXPECT_THROW(
      ReCasDigestProxyHash::deserialize(badSerializedString),
      std::invalid_argument);

  EXPECT_THROW(
      ReCasDigestProxyHash::deserialize(badSerializedString2),
      std::invalid_argument);

  EXPECT_THROW(
      ReCasDigestProxyHash::serialize(makeDigest(badHashString, size)),
      std::invalid_argument);
  EXPECT_THROW(
      ReCasDigestProxyHash::serialize(makeDigest(badHashString2, size)),
      std::invalid_argument);
}
