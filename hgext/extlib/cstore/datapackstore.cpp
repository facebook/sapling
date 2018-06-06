// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// datapackstore.cpp - implementation of a datapack store
// no-check-code

#include "hgext/extlib/cstore/datapackstore.h"

#include <stdlib.h>
#include <sys/types.h>
#include <stdexcept>

#include "hgext/extlib/cstore/key.h"
#include "lib/clib/portability/dirent.h"

namespace {

// This deleter helps us be more exception safe without needing
// to add explicit try/catch statements
struct Deleter {
  void operator()(DIR* dir) {
    closedir(dir);
  }
};

std::unordered_set<std::string> getAvailablePackFileNames(
    const std::string& path) {
  std::unordered_set<std::string> results;

  std::string packpath(path);
  if (!path.empty() && path[path.size() - 1] != '/') {
    packpath.push_back('/');
  }
  size_t dirLength = packpath.size();

  std::unique_ptr<DIR, Deleter> dirp(opendir(path.c_str()));
  if (!dirp) {
    return results;
  }

  dirent* entry;
  while ((entry = readdir(dirp.get())) != nullptr) {
    size_t fileLength = strlen(entry->d_name);
    if (fileLength < PACKSUFFIXLEN) {
      continue;
    }
    if (strcmp(entry->d_name + fileLength - PACKSUFFIXLEN, PACKSUFFIX) != 0) {
      continue;
    }
    packpath.append(entry->d_name, fileLength - PACKSUFFIXLEN);
    results.insert(packpath);
    packpath.erase(dirLength);
  }

  return results;
}
} // namespace

DatapackStore::DatapackStore(
    const std::string& path,
    bool removeDeadPackFilesOnRefresh)
    : path_(path),
      lastRefresh_(0),
      removeOnRefresh_(removeDeadPackFilesOnRefresh) {
  // Find pack files in path
  auto files = getAvailablePackFileNames(path);
  for (const auto& packpath : files) {
    addPack(packpath);
  }
}

std::shared_ptr<datapack_handle_t> DatapackStore::addPack(
    const std::string& path) {
  std::string idxPath(path + INDEXSUFFIX);
  std::string dataPath(path + PACKSUFFIX);

  std::shared_ptr<datapack_handle_t> pack(
      open_datapack(
          (char*)idxPath.c_str(),
          idxPath.size(),
          (char*)dataPath.c_str(),
          dataPath.size()),
      // set up the shared_ptr Deleter to close the datapack
      // when there are no more references
      close_datapack);
  if (pack && pack->status == DATAPACK_HANDLE_OK) {
    packs_[path] = pack;
    return pack;
  }
  return nullptr;
}

DatapackStore::~DatapackStore() {}

DeltaChainIterator DatapackStore::getDeltaChain(const Key& key) {
  std::shared_ptr<DeltaChain> chain = this->getDeltaChainRaw(key);
  if (chain->status() == GET_DELTA_CHAIN_OK) {
    return DeltaChainIterator(chain);
  }
  throw MissingKeyError("unable to find delta chain");
}

std::shared_ptr<DeltaChain> DatapackStore::getDeltaChainRaw(const Key& key) {
  for (const auto& it : packs_) {
    auto& pack = it.second;
    auto chain = getdeltachain(pack.get(), (const uint8_t*)key.node);

    if (chain.code == GET_DELTA_CHAIN_OOM) {
      throw std::runtime_error("out of memory");
    } else if (chain.code == GET_DELTA_CHAIN_NOT_FOUND) {
      freedeltachain(chain);
      continue;
    } else if (chain.code != GET_DELTA_CHAIN_OK) {
      freedeltachain(chain);
      continue;
    }

    // Pass ownership of chain to CDeltaChain
    return std::make_shared<CDeltaChain>(chain);
  }

  // Check if there are new packs available
  auto refreshed = refresh();
  for (const auto& pack : refreshed) {
    auto chain = getdeltachain(pack.get(), (const uint8_t*)key.node);
    if (chain.code == GET_DELTA_CHAIN_OOM) {
      throw std::runtime_error("out of memory");
    } else if (chain.code == GET_DELTA_CHAIN_NOT_FOUND) {
      freedeltachain(chain);
      continue;
    } else if (chain.code != GET_DELTA_CHAIN_OK) {
      freedeltachain(chain);
      continue;
    }
    // Pass ownership of chain to CDeltaChain
    return std::make_shared<CDeltaChain>(chain);
  }

  return std::make_shared<CDeltaChain>(GET_DELTA_CHAIN_NOT_FOUND);
}

Key* DatapackStoreKeyIterator::next() {
  Key* key;
  while ((key = _missing.next()) != NULL) {
    if (!_store.contains(*key)) {
      return key;
    }
  }

  return NULL;
}

bool DatapackStore::contains(const Key& key) {
  for (auto& it : packs_) {
    auto& pack = it.second;
    pack_index_entry_t packindex;
    if (find(pack.get(), (uint8_t*)key.node, &packindex)) {
      return true;
    }
  }

  // Check if there are new packs available
  auto refreshed = refresh();
  for (auto& pack : refreshed) {
    pack_index_entry_t packindex;
    if (find(pack.get(), (uint8_t*)key.node, &packindex)) {
      return true;
    }
  }

  return false;
}

std::shared_ptr<KeyIterator> DatapackStore::getMissing(KeyIterator& missing) {
  return std::make_shared<DatapackStoreKeyIterator>(*this, missing);
}

std::vector<std::shared_ptr<datapack_handle_t>> DatapackStore::refresh() {
  clock_t now = clock();

  std::vector<std::shared_ptr<datapack_handle_t>> newPacks;
  if (now - lastRefresh_ > PACK_REFRESH_RATE) {
    auto availablePacks = getAvailablePackFileNames(path_);

    // Garbage collect removed pack files
    if (removeOnRefresh_) {
      auto it = packs_.begin();
      while (it != packs_.end()) {
        if (availablePacks.find(it->first) == availablePacks.end()) {
          // This pack file no longer exists, we
          // can forget it
          it = packs_.erase(it);
          continue;
        }
        ++it;
      }
    }

    // Add any newly discovered files
    for (const auto& packPath : availablePacks) {
      if (packs_.find(packPath) == packs_.end()) {
        // We haven't loaded this path yet, do so now
        auto newPack = addPack(packPath);
        if (newPack) {
          newPacks.push_back(std::move(newPack));
        }
      }
    }

    lastRefresh_ = now;
  }

  return newPacks;
}

void DatapackStore::markForRefresh() {
  lastRefresh_ = 0;
}
