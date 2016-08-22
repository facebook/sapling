// manifest_fetcher.h - c++ declarations for a fetcher for manifests
//
// Copyright 2016 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.
//
// no-check-code

#ifndef REMOTEFILELOG_MANIFEST_FETCHER_H
#define REMOTEFILELOG_MANIFEST_FETCHER_H

#include <string>

class ManifestFetcher;

#include "manifest.h"
#include "pythonutil.h"

/**
 * A key which can be used to look up a manifest.
 */
struct manifestkey {
  std::string *path;
  std::string *node;

  manifestkey(std::string *path, std::string *node) :
      path(path),
      node(node) {
  }
};

/**
 * Class used to obtain Manifests, given a path and node.
 */
class ManifestFetcher {
  private:
    PythonObj _get;
  public:
    ManifestFetcher(PythonObj &store);

    /**
     * Fetches the Manifest from the store for the provided manifest key.
     * Returns the manifest if found, or throws an exception if not found.
     */
    Manifest *get(const manifestkey &key) const;
};

#endif //REMOTEFILELOG_MANIFEST_FETCHER_H
