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

#include <memory>
#include <string>

class ManifestFetcher;

#include "manifest.h"
#include "../cstore/store.h"

/**
 * Class used to obtain Manifests, given a path and node.
 */
class ManifestFetcher {
  private:
    std::shared_ptr<Store> _store;
  public:
    ManifestFetcher(std::shared_ptr<Store> store);

    /**
     * Fetches the Manifest from the store for the provided manifest key.
     * Returns the manifest if found, or throws an exception if not found.
     */
    ManifestPtr get(
        const char *path, size_t pathlen,
        std::string &node) const;
};

#endif //REMOTEFILELOG_MANIFEST_FETCHER_H
