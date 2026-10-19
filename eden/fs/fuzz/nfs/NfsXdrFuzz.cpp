/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <cstdint>
#include <vector>

#include <folly/Range.h>

#include "eden/fs/fuzz/nfs/NfsXdrDecode.h"
#include "eden/fs/nfs/NfsdRpc.h"
#include "eden/fs/nfs/rpc/Rpc.h"
#include "security/lionhead/utils/lib_ftest/ftest.h"

namespace {

using namespace facebook::eden;

/**
 * all_remaining_bytes is the only FDP call in each vector except nfs3_args,
 * which reads one procedure byte first, so the fuzzer's input is the XDR wire
 * payload verbatim and the dictionary entries land on real field offsets.
 */
folly::ByteRange asRange(const std::vector<uint8_t>& bytes) {
  return folly::ByteRange{bytes.data(), bytes.size()};
}

/**
 * Every NFS and mountd request enters through here: RpcServer.cpp:539 decodes
 * an rpc_msg_call off the socket before it knows which procedure it is holding.
 * The two opaque_auth bodies it contains are length-prefixed
 * std::vector<uint8_t>, the trait that reads a uint32_t length and sizes an
 * allocation from it before the cursor can prove the payload backs it.
 */
FUZZ(NfsXdr, rpc_call_message) {
  auto wire = f.all_remaining_bytes();
  decodeXdr<rpc_msg_call>(asRange(wire));
}

/**
 * LOOKUP is the highest-volume NFSv3 procedure. diropargs3 pairs the nfs_fh3
 * handle-size check, which throws RpcParsingError on a mismatch, with a
 * length-prefixed std::string read through readFixedString.
 */
FUZZ(NfsXdr, lookup_args) {
  auto wire = f.all_remaining_bytes();
  decodeXdr<LOOKUP3args>(asRange(wire));
}

/**
 * Mirrors the procedure-selection chain in Nfsd3ServerProcessor::dispatchRpc:
 * `call_body::proc` indexes kNfs3dHandlers and the chosen handler
 * deserializes its own argument struct off the cursor. The first input byte
 * is that procedure number and the rest is the payload, so the fuzzer drives
 * the dispatch on `nfsv3Procs` and the four decoders behind it together, and
 * a mutation that flips the proc byte re-reads one payload as another struct,
 * instead of four separate harnesses each pinned to one struct. READ3args is
 * fixed-width behind the handle; WRITE3args carries a length-prefixed IOBuf
 * that clones out of the cursor rather than copying; LOOKUP3args and
 * CREATE3args are also covered on their own because they are the two with a
 * string read.
 */
FUZZ(NfsXdr, nfs3_args) {
  auto proc = f.u8("proc");
  auto wire = f.all_remaining_bytes();
  decodeNfs3Args(proc, asRange(wire));
}

/**
 * CREATE3args is the deepest of the argument structs: a diropargs3 followed by
 * createhow3, a discriminated union that switches into either sattr3 with its
 * six nested optional variants or a fixed-size createverf3 array.
 */
FUZZ(NfsXdr, create_args) {
  auto wire = f.all_remaining_bytes();
  decodeXdr<CREATE3args>(asRange(wire));
}

} // namespace
