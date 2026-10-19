/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <cstdint>
#include <string>

#include <folly/Range.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <folly/io/IOBufQueue.h>
#include <gtest/gtest.h>

#include "eden/fs/fuzz/nfs/NfsXdrDecode.h"
#include "eden/fs/nfs/NfsdRpc.h"
#include "eden/fs/nfs/rpc/Rpc.h"

namespace {

using namespace facebook::eden;

constexpr size_t kAppenderGrowth = 1024;

template <typename T>
std::string encode(const T& value) {
  folly::IOBufQueue queue;
  folly::io::QueueAppender appender(&queue, kAppenderGrowth);
  XdrTrait<T>::serialize(appender, value);
  auto buf = queue.move();
  if (!buf) {
    return {};
  }
  auto range = buf->coalesce();
  return std::string(reinterpret_cast<const char*>(range.data()), range.size());
}

folly::ByteRange asRange(const std::string& bytes) {
  return folly::ByteRange{
      reinterpret_cast<const uint8_t*>(bytes.data()), bytes.size()};
}

template <typename T>
T decodeToEnd(const std::string& wire) {
  auto buf = folly::IOBuf::wrapBufferAsValue(asRange(wire));
  folly::io::Cursor cursor(&buf);
  auto value = XdrTrait<T>::deserialize(cursor);
  EXPECT_TRUE(cursor.isAtEnd());
  return value;
}

/**
 * Overwrite the big-endian uint32_t length prefix at `offset` so it claims more
 * bytes than the message actually carries.
 */
void setLengthPrefix(std::string& wire, size_t offset, uint32_t length) {
  ASSERT_LE(offset + 4, wire.size());
  wire[offset] = static_cast<char>((length >> 24) & 0xff);
  wire[offset + 1] = static_cast<char>((length >> 16) & 0xff);
  wire[offset + 2] = static_cast<char>((length >> 8) & 0xff);
  wire[offset + 3] = static_cast<char>(length & 0xff);
}

rpc_msg_call validCall() {
  return rpc_msg_call{
      0x1234abcd,
      msg_type::CALL,
      call_body{
          kRPCVersion,
          kNfsdProgNumber,
          kNfsd3ProgVersion,
          static_cast<uint32_t>(nfsv3Procs::lookup),
          opaque_auth{auth_flavor::AUTH_SYS, OpaqueBytes{1, 2, 3, 4, 5}},
          opaque_auth{auth_flavor::AUTH_NONE, OpaqueBytes{}},
      },
  };
}

LOOKUP3args validLookupArgs() {
  return LOOKUP3args{diropargs3{nfs_fh3{InodeNumber{42}}, "readme.txt"}};
}

READ3args validReadArgs() {
  return READ3args{nfs_fh3{InodeNumber{7}}, 4096, 65536};
}

WRITE3args validWriteArgs() {
  return WRITE3args{
      nfs_fh3{InodeNumber{9}},
      1024,
      11,
      stable_how::FILE_SYNC,
      folly::IOBuf::copyBuffer("hello world")};
}

CREATE3args validCreateArgs() {
  return CREATE3args{
      diropargs3{nfs_fh3{InodeNumber{3}}, "newfile"},
      createhow3{{createmode3::UNCHECKED, sattr3{}}}};
}

CREATE3args validExclusiveCreateArgs() {
  return CREATE3args{
      diropargs3{nfs_fh3{InodeNumber{3}}, "newfile"},
      createhow3{
          {createmode3::EXCLUSIVE, createverf3{{1, 2, 3, 4, 5, 6, 7, 8}}}}};
}

// The seeds are real messages, not merely bytes the decoder tolerates: every
// field survives the round trip and the cursor lands exactly at the end.
TEST(NfsXdrSeed, wellFormedMessagesRoundTrip) {
  EXPECT_EQ(validCall(), decodeToEnd<rpc_msg_call>(encode(validCall())));
  EXPECT_EQ(
      validLookupArgs(), decodeToEnd<LOOKUP3args>(encode(validLookupArgs())));
  EXPECT_EQ(validReadArgs(), decodeToEnd<READ3args>(encode(validReadArgs())));
  EXPECT_EQ(
      validCreateArgs(), decodeToEnd<CREATE3args>(encode(validCreateArgs())));
  EXPECT_EQ(
      validExclusiveCreateArgs(),
      decodeToEnd<CREATE3args>(encode(validExclusiveCreateArgs())));
}

// WRITE3args holds its payload in a unique_ptr<IOBuf>, so operator== compares
// pointers rather than bytes. Check the cloned payload directly instead.
TEST(NfsXdrSeed, wellFormedWriteArgsRoundTrips) {
  // The decoded IOBuf clones out of the cursor rather than copying, so the
  // encoded bytes have to outlive it.
  auto wire = encode(validWriteArgs());
  auto decoded = decodeToEnd<WRITE3args>(wire);
  EXPECT_EQ(nfs_fh3{InodeNumber{9}}, decoded.file);
  EXPECT_EQ(1024u, decoded.offset);
  EXPECT_NE(nullptr, decoded.data);
  if (!decoded.data) {
    return;
  }
  auto payload = decoded.data->coalesce();
  EXPECT_EQ(
      "hello world",
      std::string(
          reinterpret_cast<const char*>(payload.data()), payload.size()));
}

// The depth proof for the harness itself: the same seeds, through the same
// decodeXdr the FUZZ vectors call, reach Parsed rather than being rejected at
// the first field.
TEST(NfsXdrSeed, harnessDecodesWellFormedSeeds) {
  EXPECT_EQ(
      XdrDecodeOutcome::Parsed,
      decodeXdr<rpc_msg_call>(asRange(encode(validCall()))));
  EXPECT_EQ(
      XdrDecodeOutcome::Parsed,
      decodeXdr<LOOKUP3args>(asRange(encode(validLookupArgs()))));
  EXPECT_EQ(
      XdrDecodeOutcome::Parsed,
      decodeXdr<READ3args>(asRange(encode(validReadArgs()))));
  EXPECT_EQ(
      XdrDecodeOutcome::Parsed,
      decodeXdr<WRITE3args>(asRange(encode(validWriteArgs()))));
  EXPECT_EQ(
      XdrDecodeOutcome::Parsed,
      decodeXdr<CREATE3args>(asRange(encode(validCreateArgs()))));
  EXPECT_EQ(
      XdrDecodeOutcome::Parsed,
      decodeXdr<CREATE3args>(asRange(encode(validExclusiveCreateArgs()))));
}

// The depth proof for nfs3_args: the procedure byte selects the decoder the
// real dispatch would, and each of the four well-formed payloads reaches Parsed
// through it.
TEST(NfsXdrSeed, nfs3ArgsDispatchOnProcedureDecodesWellFormedSeeds) {
  auto proc = [](nfsv3Procs p) { return static_cast<uint8_t>(p); };
  EXPECT_EQ(
      XdrDecodeOutcome::Parsed,
      decodeNfs3Args(
          proc(nfsv3Procs::lookup), asRange(encode(validLookupArgs()))));
  EXPECT_EQ(
      XdrDecodeOutcome::Parsed,
      decodeNfs3Args(proc(nfsv3Procs::read), asRange(encode(validReadArgs()))));
  auto write = encode(validWriteArgs());
  EXPECT_EQ(
      XdrDecodeOutcome::Parsed,
      decodeNfs3Args(proc(nfsv3Procs::write), asRange(write)));
  EXPECT_EQ(
      XdrDecodeOutcome::Parsed,
      decodeNfs3Args(
          proc(nfsv3Procs::create), asRange(encode(validCreateArgs()))));
  EXPECT_EQ(
      XdrDecodeOutcome::Parsed,
      decodeNfs3Args(
          proc(nfsv3Procs::create),
          asRange(encode(validExclusiveCreateArgs()))));
}

// A procedure with no decoder here returns NoDecoder without reading the
// payload: an empty payload, which every decoder rejects, is not even looked
// at. GETATTR is a real procedure outside the set; 0xff is not a procedure.
TEST(NfsXdrSeed, nfs3ArgsUnknownProcedureSelectsNoDecoder) {
  EXPECT_EQ(
      XdrDecodeOutcome::NoDecoder,
      decodeNfs3Args(
          static_cast<uint8_t>(nfsv3Procs::getattr), folly::ByteRange{}));
  EXPECT_EQ(
      XdrDecodeOutcome::NoDecoder, decodeNfs3Args(0xff, folly::ByteRange{}));
  EXPECT_EQ(
      XdrDecodeOutcome::Rejected,
      decodeNfs3Args(
          static_cast<uint8_t>(nfsv3Procs::lookup), folly::ByteRange{}));
}

// ERR_LH_001: a zero-byte input must be rejected rather than crash.
TEST(NfsXdrSeed, emptyInputIsRejected) {
  EXPECT_EQ(
      XdrDecodeOutcome::Rejected, decodeXdr<rpc_msg_call>(folly::ByteRange{}));
  EXPECT_EQ(
      XdrDecodeOutcome::Rejected, decodeXdr<LOOKUP3args>(folly::ByteRange{}));
  EXPECT_EQ(
      XdrDecodeOutcome::Rejected, decodeXdr<READ3args>(folly::ByteRange{}));
  EXPECT_EQ(
      XdrDecodeOutcome::Rejected, decodeXdr<WRITE3args>(folly::ByteRange{}));
  EXPECT_EQ(
      XdrDecodeOutcome::Rejected, decodeXdr<CREATE3args>(folly::ByteRange{}));
}

TEST(NfsXdrSeed, truncatedMessagesAreRejected) {
  auto call = encode(validCall());
  ASSERT_GT(call.size(), 4u);
  EXPECT_EQ(
      XdrDecodeOutcome::Rejected,
      decodeXdr<rpc_msg_call>(asRange(call.substr(0, call.size() - 4))));

  auto lookup = encode(validLookupArgs());
  ASSERT_GT(lookup.size(), 2u);
  EXPECT_EQ(
      XdrDecodeOutcome::Rejected,
      decodeXdr<LOOKUP3args>(asRange(lookup.substr(0, lookup.size() - 2))));

  auto write = encode(validWriteArgs());
  ASSERT_GT(write.size(), 6u);
  EXPECT_EQ(
      XdrDecodeOutcome::Rejected,
      decodeXdr<WRITE3args>(asRange(write.substr(0, write.size() - 6))));
}

// A length prefix that overruns the payload must be rejected, not trusted. The
// opaque_auth cred body is the first such prefix in an RPC call: 4 bytes of xid
// + 4 mtype + 16 of call_body's fixed fields + 4 of auth flavor puts it at 28.
TEST(NfsXdrSeed, oversizedLengthPrefixIsRejected) {
  auto call = encode(validCall());
  setLengthPrefix(call, 28, 4096);
  EXPECT_EQ(XdrDecodeOutcome::Rejected, decodeXdr<rpc_msg_call>(asRange(call)));

  // diropargs3's name length sits after the 4-byte handle size and the 8-byte
  // inode number.
  auto lookup = encode(validLookupArgs());
  setLengthPrefix(lookup, 12, 4096);
  EXPECT_EQ(
      XdrDecodeOutcome::Rejected, decodeXdr<LOOKUP3args>(asRange(lookup)));
}

// nfs_fh3 rejects any handle whose declared size is not the 8 bytes EdenFS
// writes, which is the gate every argument struct sits behind.
TEST(NfsXdrSeed, wrongHandleSizeIsRejected) {
  auto lookup = encode(validLookupArgs());
  setLengthPrefix(lookup, 0, 16);
  EXPECT_EQ(
      XdrDecodeOutcome::Rejected, decodeXdr<LOOKUP3args>(asRange(lookup)));
}

} // namespace
