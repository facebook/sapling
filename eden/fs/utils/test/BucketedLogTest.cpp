/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "eden/fs/utils/BucketedLog.h"

#include <folly/test/TestUtils.h>
#include <gtest/gtest.h>

using namespace facebook::eden;

namespace {

struct Bucket {
  std::string s;

  void add(const std::string& t) {
    s += t;
  }

  void merge(const Bucket& other) {
    s += other.s;
  }

  void clear() {
    s.clear();
  }

  bool operator==(const Bucket& other) const {
    return s == other.s;
  }
  bool operator!=(const Bucket& other) const {
    return s != other.s;
  }
};

template <typename T, typename... U>
std::array<Bucket, 1 + sizeof...(U)> bucketArray(T&& t, U&&... u) {
  return {Bucket{std::forward<T>(t)}, Bucket{std::forward<U>(u)}...};
}

std::ostream& operator<<(std::ostream& os, const Bucket& bucket) {
  return os << '"' << bucket.s << '"';
}

template <size_t N>
std::ostream& operator<<(
    std::ostream& os,
    const std::array<Bucket, N>& buckets) {
  os << "{";
  bool first = true;
  for (auto& entry : buckets) {
    if (!first) {
      os << ", ";
    }
    os << entry;
    first = false;
  }
  return os << "}";
}

template <size_t N>
void PrintTo(const std::array<Bucket, N>& buckets, std::ostream* os) {
  (*os) << buckets;
}

} // namespace

TEST(BucketedLog, drops_values_too_old) {
  BucketedLog<Bucket, 3> b;

  b.add(1, "a");
  EXPECT_EQ(bucketArray("", "", "a"), b.getAll(1));

  b.add(2, "b");
  EXPECT_EQ(bucketArray("", "a", "b"), b.getAll(2));

  b.add(3, "c");
  EXPECT_EQ(bucketArray("a", "b", "c"), b.getAll(3));

  b.add(4, "d");
  EXPECT_EQ(bucketArray("b", "c", "d"), b.getAll(4));
}

TEST(BucketedLog, accumulates_within_bucket) {
  BucketedLog<Bucket, 3> b;
  b.add(1, "a");
  b.add(1, "b");
  b.add(1, "c");
  EXPECT_EQ(bucketArray("", "", "abc"), b.getAll(1));
}

TEST(BucketedLog, drops_old_values_when_time_skips_ahead) {
  BucketedLog<Bucket, 3> b;
  b.add(1, "a");
  b.add(4, "b");
  b.add(7, "c");
  EXPECT_EQ(bucketArray("", "", "c"), b.getAll(7));
  EXPECT_EQ(bucketArray("", "", ""), b.getAll(10));
}

TEST(BucketedLog, merge_at_zero) {
  BucketedLog<Bucket, 3> b1;
  BucketedLog<Bucket, 3> b2;
  b1.add(0, "a");
  b2.add(0, "b");

  b2.merge(b1);
  EXPECT_EQ(bucketArray("", "", "ba"), b2.getAll(0));
}

TEST(BucketedLog, merging_into_empty_equals_original) {
  BucketedLog<Bucket, 3> b1;
  b1.add(1, "a");
  b1.add(4, "b");
  b1.add(6, "c");

  BucketedLog<Bucket, 3> b2;
  b2.merge(b1);

  EXPECT_EQ(bucketArray("b", "", "c"), b2.getAll(6));
}

TEST(BucketedLog, merge_drops_old_records) {
  BucketedLog<Bucket, 3> b1;
  BucketedLog<Bucket, 3> b2;

  // Offset b1 and b2 from each other and have them each drop a bucket.
  b1.add(1, "a");
  b1.add(2, "b");
  b1.add(3, "c");
  b1.add(4, "d");

  b2.add(2, "e");
  b2.add(3, "f");
  b2.add(4, "g");
  b2.add(5, "h");

  // Test merging both into an empty BucketedLog...
  BucketedLog<Bucket, 3> b3;
  b3.merge(b2);
  b3.merge(b1);
  EXPECT_EQ(bucketArray("fc", "gd", "h"), b3.getAll(5));

  // And merging one into the other...
  b2.merge(b1);
  EXPECT_EQ(bucketArray("fc", "gd", "h"), b2.getAll(5));
}

TEST(BucketedLog, keeps_older_data_points_but_drops_expired_ones) {
  BucketedLog<Bucket, 3> b;
  b.add(2, "a");
  b.add(3, "b");
  b.add(4, "c");

  b.add(3, "d");
  b.add(1, "e");
  EXPECT_EQ(bucketArray("a", "bd", "c"), b.getAll(4));
}
