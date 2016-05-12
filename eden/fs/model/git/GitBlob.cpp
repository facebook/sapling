/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "GitBlob.h"
#include <folly/Format.h>
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"

using folly::ByteRange;
using folly::IOBuf;
using folly::StringPiece;
using std::invalid_argument;

namespace facebook {
namespace eden {

namespace {
/** This is a helper function for releasing memory that we transfer into an
 * IOBuf.  It matches the signature of IOBuf::FreeFunction.  The first
 * parameter points to the start of the blob content, but is uninteresting
 * here, so we omit it.  userData is the pointer to the string that we donated
 * to the IOBuf and that we are responsible for deleting now. */
void freeString(void*, void* userData) {
  auto str = static_cast<std::string*>(userData);
  delete str;
}
}

std::unique_ptr<Blob> deserializeGitBlob(
    const Hash& hash,
    std::string&& gitBlobObject) {
  auto blob_pointer = std::make_unique<std::string>(std::move(gitBlobObject));

  // Find the end of the header and extract the size.
  constexpr StringPiece prefix("blob ");

  StringPiece blobChars{*blob_pointer};
  if (!blobChars.startsWith(prefix)) {
    throw invalid_argument("Contents did not start with expected header.");
  }

  // Remember the initial data pointer for later.
  StringPiece origBlobChars{blobChars};

  // Skip the prefix.
  blobChars.advance(prefix.size());

  // Parse the content size; it is a textual integer.
  // This advances blobChars to after the integer.
  auto contentSize = folly::to<unsigned int>(&blobChars);
  if (blobChars.at(0) != '\0') {
    throw invalid_argument("Header should be followed by NUL.");
  }

  // Walk over the NUL.
  blobChars.advance(1);

  // Now blobChars refers to just the content portion.
  // Sanity check that the size is correct.
  if (contentSize != blobChars.size()) {
    throw invalid_argument("Size in header should match contents");
  }

  // We know enough to safely proceed; move the string into an IOBuf.
  // This is slightly tricky; we inform the IOBuf about the readable
  // data range from blobChars and transfer ownership of the storage
  // such that it will call freeString later on.
  auto iobuf = IOBuf::takeOwnership(
      const_cast<char*>(blobChars.data()),
      blobChars.size(),
      freeString,
      // A weak move of the pointer... freeString will delete it
      // at the appropriate time.
      blob_pointer.release());

  // and now we can create the blob!
  return std::make_unique<Blob>(hash, std::move(*iobuf));
}

std::unique_ptr<Blob> deserializeGitBlob(
    const Hash& hash,
    ByteRange gitBlobObject) {
  return deserializeGitBlob(hash, StringPiece{gitBlobObject}.str());
}
}
}
