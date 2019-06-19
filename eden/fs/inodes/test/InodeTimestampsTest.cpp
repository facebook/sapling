/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "eden/fs/inodes/InodeTimestamps.h"

#include <folly/Portability.h>
#include <gtest/gtest.h>

using namespace facebook::eden;

// TODO: make these into runtime checks and skip the test if unsuccessful.

static void require_signed_time_t() {
  static_assert(
      std::is_signed<decltype(timespec().tv_sec)>::value,
      "this test requires signed time_t");
}
static void require_64bit_time_t() {
  static_assert(
      sizeof(timespec().tv_sec) > 4, "this test requires 64-bit time_t");
}

static inline uint64_t rawRep(EdenTimestamp ts) {
  return ts.asRawRepresentation();
}

TEST(EdenTimestamp, zero_timespec_is_unix_epoch) {
  auto ts = timespec{};
  auto et = EdenTimestamp{ts};

  EXPECT_EQ(0x80000000ull * 1000000000ull, rawRep(et));
  EXPECT_EQ(0, et.toTimespec().tv_sec);
  EXPECT_EQ(0, et.toTimespec().tv_nsec);
}

TEST(EdenTimestamp, round_trip_shortly_after_epoch) {
  timespec ts1;
  ts1.tv_sec = 1;
  ts1.tv_nsec = 100;

  auto ts2 = EdenTimestamp{ts1}.toTimespec();
  EXPECT_EQ(ts1.tv_sec, ts2.tv_sec);
  EXPECT_EQ(ts1.tv_nsec, ts2.tv_nsec);
}

TEST(EdenTimestamp, round_trip_shortly_before_epoch) {
  timespec ts1;
  ts1.tv_sec = -1;
  ts1.tv_nsec = 100;

  auto ts2 = EdenTimestamp{ts1}.toTimespec();
  EXPECT_EQ(ts1.tv_sec, ts2.tv_sec);
  EXPECT_EQ(ts1.tv_nsec, ts2.tv_nsec);
}

TEST(EdenTimestamp, earliest_possible_value) {
  require_signed_time_t();

  timespec ts;
  ts.tv_sec = -0x80000000ll;
  ts.tv_nsec = 0;
  auto et = EdenTimestamp{ts};
  EXPECT_EQ(-0x80000000ll, ts.tv_sec);

  EXPECT_EQ(0ull, rawRep(et));
  EXPECT_EQ(-0x80000000ll, et.toTimespec().tv_sec);
  EXPECT_EQ(0, et.toTimespec().tv_nsec);
}

TEST(EdenTimestamp, latest_possible_value) {
  require_64bit_time_t();

  timespec ts;
  ts.tv_sec = 16299260425ull;
  ts.tv_nsec = 709551615;
  const auto et = EdenTimestamp{ts};

  EXPECT_EQ(~0ull, rawRep(EdenTimestamp{ts}));
  EXPECT_EQ(ts.tv_sec, et.toTimespec().tv_sec);
  EXPECT_EQ(ts.tv_nsec, et.toTimespec().tv_nsec);

  // verify round-tripping through one nsec less than the largest value
  ts.tv_nsec -= 1;
  const auto et2 = EdenTimestamp{ts};
  EXPECT_EQ(~0ull - 1, rawRep(et2));
  EXPECT_EQ(ts.tv_sec, et2.toTimespec().tv_sec);
  EXPECT_EQ(ts.tv_nsec, et2.toTimespec().tv_nsec);
}

TEST(EdenTimestamp, clamps_to_earliest_value) {
  require_64bit_time_t();
  require_signed_time_t();
  timespec ts;
  ts.tv_sec = -0x80000001ull;
  ts.tv_nsec = 0;
  EXPECT_EQ(0ull, rawRep(EdenTimestamp{ts}));
}

TEST(EdenTimestamp, clamps_to_latest_value) {
  require_64bit_time_t();

  timespec latest;
  latest.tv_sec = 16299260425ull;
  latest.tv_nsec = 709551615;

  timespec latest_plus_1s{latest};
  ++latest_plus_1s.tv_sec;

  timespec latest_plus_1ns{latest};
  ++latest_plus_1ns.tv_nsec;

  auto et1 = EdenTimestamp{latest};
  auto et2 = EdenTimestamp{latest_plus_1s};
  auto et3 = EdenTimestamp{latest_plus_1ns};

  EXPECT_EQ(rawRep(et1), rawRep(et2));
  EXPECT_EQ(rawRep(et1), rawRep(et3));
}

TEST(EdenTimestamp, throws_on_underflow_if_desired) {
  require_64bit_time_t();
  require_signed_time_t();
  timespec ts;
  ts.tv_sec = -0x80000001ull;
  ts.tv_nsec = 0;
  EXPECT_THROW(
      (EdenTimestamp{ts, EdenTimestamp::throwIfOutOfRange}),
      std::underflow_error);
}

TEST(EdenTimestamp, throws_on_overflow_if_desired) {
  require_64bit_time_t();

  timespec latest;
  latest.tv_sec = 16299260425ull;
  latest.tv_nsec = 709551615;

  timespec latest_plus_1s{latest};
  ++latest_plus_1s.tv_sec;

  timespec latest_plus_1ns{latest};
  ++latest_plus_1ns.tv_nsec;

  EXPECT_THROW(
      (EdenTimestamp{latest_plus_1s, EdenTimestamp::throwIfOutOfRange}),
      std::overflow_error);
  EXPECT_THROW(
      (EdenTimestamp{latest_plus_1ns, EdenTimestamp::throwIfOutOfRange}),
      std::overflow_error);
}

template <int By>
inline uint64_t shl(uint64_t u) {
  return (By < 0) ? u >> -By : u << By;
}

TEST(EdenTimestamp, semi_exhaustive_round_trip) {
  constexpr int kIterationBits = folly::kIsDebug ? 17 : 23;
  for (unsigned u = 0; u < (1u << kIterationBits); ++u) {
    // spread the bits out evenly
    uint64_t nsec = shl<64 - kIterationBits>(u) |
        shl<64 - kIterationBits * 2>(u) | shl<64 - kIterationBits * 3>(u) |
        shl<64 - kIterationBits * 4>(u);
    auto et1 = EdenTimestamp{nsec};
    auto ts1 = et1.toTimespec();
    auto et2 = EdenTimestamp{ts1, EdenTimestamp::throwIfOutOfRange};
    auto ts2 = et2.toTimespec();
    ASSERT_EQ(rawRep(et1), rawRep(et2))
        << "while testing value u=" << u << " nsec=" << nsec;
    ASSERT_EQ(ts1.tv_sec, ts2.tv_sec)
        << "while testing value u=" << u << " nsec=" << nsec;
    ASSERT_EQ(ts1.tv_nsec, ts2.tv_nsec)
        << "while testing value u=" << u << " nsec=" << nsec;
  }
}
