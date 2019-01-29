// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// manifest_entry.h - c++ declaration for a single manifest entry
// no-check-code

#ifndef FBHGEXT_CTREEMANIFEST_MANIFEST_ENTRY_H
#define FBHGEXT_CTREEMANIFEST_MANIFEST_ENTRY_H

#include <cstddef>
#include <cstring>
#include <string>

#include "edenscm/hgext/extlib/ctreemanifest/manifest_fetcher.h"
#include "edenscm/hgext/extlib/ctreemanifest/manifest_ptr.h"
#include "lib/clib/convert.h"

#define MANIFEST_DIRECTORY_FLAG 't'
#define MANIFEST_DIRECTORY_FLAGPTR (&"t"[0])

/**
 * Class representing a single entry in a given manifest.  Instances of this
 * class may refer to that it does not own.  If it owns any memory, it is a
 * single block referenced by the ownedmemory field.
 */
class ManifestEntry {
 private:
  const char* node;

 public:
  const char* filename;
  size_t filenamelen;

  // unlike filename/node, this is not always a valid pointer.  if the flag
  // is unset, flag will be set to NULL.
  const char* flag;
  ManifestPtr resolved;
  char* ownedmemory;

  // TODO: add hint storage here as well

  ManifestEntry();

  ~ManifestEntry();

  bool isdirectory() const;
  bool hasNode() const;
  const char* get_node();
  void reset_node();

  void appendtopath(std::string& path);

  ManifestPtr get_manifest(
      const ManifestFetcher& fetcher,
      const char* path,
      size_t pathlen);

  void initialize(
      const char* filename,
      const size_t filenamelen,
      const char* node,
      const char* flag);

  const char* initialize(const char* entrystart);

  void initialize(ManifestEntry* other);

  void updatebinnode(const char* node, const char* flag);
  void updatehexnode(const char* node, const char* flag);

  /**
   * Returns true iff the left precedes right.
   */
  static bool compareMercurialOrder(
      ManifestEntry* const& left,
      ManifestEntry* const& right);

  /**
   * Compares the name of two entries. This is useful when
   * iterating through ManifestEntries simultaneously.
   */
  static int compareName(ManifestEntry* left, ManifestEntry* right);
};

#endif // FBHGEXT_CTREEMANIFEST_MANIFEST_ENTRY_H
