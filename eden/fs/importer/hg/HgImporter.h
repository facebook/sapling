/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include <folly/Range.h>
#include <folly/Subprocess.h>

namespace folly {
namespace io {
class Cursor;
}
}

namespace facebook {
namespace eden {

class Hash;
class HgManifestImporter;
class LocalStore;

/*
 * HgImporter is the main class for all mercurial import functionality.
 */
class HgImporter {
 public:
  /*
   * Create a new HgImporter object that will import data from the specified
   * repository into the given LocalStore.
   *
   * The caller is responsible for ensuring that the LocalStore object remains
   * valid for the lifetime of the HgImporter object.
   */
  HgImporter(folly::StringPiece repoPath, LocalStore* store);
  virtual ~HgImporter();

  /**
   * Import the manifest for the specified revision.
   *
   * Returns a Hash identifying the root Tree for the imported revision.
   */
  Hash importManifest(folly::StringPiece revName);

 private:
  /*
   * Chunk header flags.
   *
   * These are flag values, designed to be bitwise ORed with each other.
   */
  enum : uint32_t {
    FLAG_ERROR = 0x01,
    FLAG_MORE_CHUNKS = 0x02,
  };
  /*
   * Command type values.
   */
  enum : uint32_t {
    CMD_RESPONSE = 0,
    CMD_MANIFEST = 1,
  };
  struct ChunkHeader {
    uint32_t requestID;
    uint32_t command;
    uint32_t flags;
    uint32_t dataLength;
  };

  // Forbidden copy constructor and assignment operator
  HgImporter(const HgImporter&) = delete;
  HgImporter& operator=(const HgImporter&) = delete;

  /*
   * Read a single manifest entry from a manifest response chunk,
   * and give it to the HgManifestImporter for processing.
   *
   * The cursor argument points to the start of the manifest entry in the
   * response chunk received from the helper process.  readManifestEntry() is
   * responsible for updating the cursor to point to the next manifest entry.
   */
  void readManifestEntry(
      HgManifestImporter& importer,
      folly::io::Cursor& cursor);
  /*
   * Read a response chunk header from the helper process
   */
  ChunkHeader readChunkHeader();
  /*
   * Send a request to the helper process, asking it to send us the manifest
   * for the specified revision.
   */
  void sendManifestRequest(folly::StringPiece revName);

  folly::Subprocess helper_;
  LocalStore* store_{nullptr};
  uint32_t nextRequestID_{0};
};
}
} // facebook::eden
