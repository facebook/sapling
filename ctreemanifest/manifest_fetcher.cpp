// manifest_fetcher.cpp - c++ implementation of a fetcher for manifests
//
// Copyright 2016 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.
//
// no-check-code

#include "manifest_fetcher.h"

ManifestFetcher::ManifestFetcher(std::shared_ptr<Store> store) :
  _store(store) {
}

/**
 * Fetches the Manifest from the store for the provided manifest key.
 * Returns the manifest if found, or throws an exception if not found.
 */
ManifestPtr ManifestFetcher::get(
    const char *path, size_t pathlen,
    std::string &node) const {
    ConstantStringRef content = _store->get(Key(path, pathlen, node.c_str(), node.size()));
    return ManifestPtr(new Manifest(content));
}
