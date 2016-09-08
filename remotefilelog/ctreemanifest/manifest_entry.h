// manifest_entry.h - c++ declaration for a single manifest entry
//
// Copyright 2016 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.
//
// no-check-code

#ifndef REMOTEFILELOG_MANIFEST_ENTRY_H
#define REMOTEFILELOG_MANIFEST_ENTRY_H

#include "pythonutil.h"

#include <cstddef>
#include <cstring>
#include <string>

class ManifestEntry;

#include "convert.h"
#include "manifest.h"
#include "manifest_fetcher.h"

#define MANIFEST_DIRECTORY_FLAG 't'

/**
 * Class representing a single entry in a given manifest.  Instances of this
 * class may refer to that it does not own.  If it owns any memory, it is a
 * single block referenced by the ownedmemory field.
 */
class ManifestEntry {
  public:
    char *filename;
    size_t filenamelen;
    char *node;

    // unlike filename/node, this is not always a valid pointer.  if the flag
    // is unset, flag will be set to NULL.
    char *flag;
    Manifest *resolved;
    char *ownedmemory;

    // TODO: add hint storage here as well

    ManifestEntry();

    /**
     * Given the start of a file/dir entry in a manifest, returns a
     * ManifestEntry structure with the parsed data.
     */
    ManifestEntry(char *&entrystart);

    ~ManifestEntry();

    bool isdirectory() const;

    void appendtopath(std::string &path);

    Manifest *get_manifest(
        ManifestFetcher fetcher, const char *path, size_t pathlen);

    void initialize(
        const char *filename, const size_t filenamelen,
        const char *node,
        char flag);

    void update(const char *node, char flag);
};

#endif //REMOTEFILELOG_MANIFEST_ENTRY_H
