/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <cstdint>
#include <exception>

#include <folly/Range.h>
#include <folly/Utility.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <folly/lang/Hint.h>

#include "eden/fs/nfs/NfsdRpc.h"
#include "eden/fs/nfs/xdr/Xdr.h"

namespace facebook::eden {

enum class XdrDecodeOutcome {
  /// The cursor yielded a complete value of type T.
  Parsed,
  /// The decoder threw. Malformed input is supposed to be rejected this way,
  /// so this is an expected outcome and not a finding.
  Rejected,
  /// The procedure number selected no argument decoder; nothing was read.
  NoDecoder,
};

/**
 * Decode a single XDR message of type T out of `wire`, which is the byte
 * sequence a folly::io::Cursor sees coming off the NFS socket.
 *
 * The fuzz harness and its seed test both go through this function, so a seed
 * the test proves reaches Parsed is a seed the harness decodes to the same
 * depth.
 *
 * Nothing about the decoded value is inspected; ASAN and UBSAN are the oracle.
 * serializedSize is called only to keep the decoded fields live: every
 * XdrTrait::deserialize is header-inlined and its results are otherwise dead,
 * which would let the optimizer drop the reads under test.
 */
template <typename T>
XdrDecodeOutcome decodeXdr(folly::ByteRange wire) {
  // An empty input hands us a null pointer, which IOBuf will not wrap.
  static constexpr uint8_t kEmpty[1] = {0};
  auto buf = folly::IOBuf::wrapBufferAsValue(
      wire.data() ? wire.data() : kEmpty, wire.size());
  folly::io::Cursor cursor(&buf);
  try {
    auto value = XdrTrait<T>::deserialize(cursor);
    folly::compiler_must_not_elide(XdrTrait<T>::serializedSize(value));
    return XdrDecodeOutcome::Parsed;
  } catch (const std::exception&) {
    return XdrDecodeOutcome::Rejected;
  }
}

/**
 * Decode the argument struct of one NFSv3 procedure the way
 * Nfsd3ServerProcessor::dispatchRpc selects it: `proc` is `call_body::proc`,
 * which Nfsd3.cpp indexes into kNfs3dHandlers, and `args` is the XDR payload
 * the chosen handler deserializes off the cursor. Only LOOKUP, READ, WRITE
 * and CREATE have a decoder here; every other value is NoDecoder and leaves
 * `args` unread.
 */
inline XdrDecodeOutcome decodeNfs3Args(uint8_t proc, folly::ByteRange args) {
  switch (proc) {
    case folly::to_underlying(nfsv3Procs::lookup):
      return decodeXdr<LOOKUP3args>(args);
    case folly::to_underlying(nfsv3Procs::read):
      return decodeXdr<READ3args>(args);
    case folly::to_underlying(nfsv3Procs::write):
      return decodeXdr<WRITE3args>(args);
    case folly::to_underlying(nfsv3Procs::create):
      return decodeXdr<CREATE3args>(args);
    default:
      return XdrDecodeOutcome::NoDecoder;
  }
}

} // namespace facebook::eden
