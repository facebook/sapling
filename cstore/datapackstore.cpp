// datapackstore.cpp - implementation of a datapack store
//
// Copyright 2017 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.
//
// no-check-code

#include "datapackstore.h"
#include "key.h"

#include <sys/types.h>
#include <dirent.h>
#include <stdexcept>
#include <stdlib.h>

std::vector<std::string> getAvailablePackFiles(const std::string &path) {
  std::vector<std::string> results;

  std::string packpath(path);
  if (!path.empty() && path[path.size() - 1] != '/') {
    packpath.push_back('/');
  }
  size_t dirLength = packpath.size();

  DIR *dirp = opendir(path.c_str());
  if (!dirp) {
    return results;
  }

  try {
    dirent *entry;
    while ((entry = readdir(dirp)) != NULL) {
      size_t fileLength = strlen(entry->d_name);
      if (fileLength < PACKSUFFIXLEN) {
        continue;
      }

      if (strcmp(entry->d_name + fileLength - PACKSUFFIXLEN, PACKSUFFIX) != 0) {
        continue;
      }

      packpath.append(entry->d_name, fileLength - PACKSUFFIXLEN);
      results.push_back(packpath);
      packpath.erase(dirLength);
    }

    closedir(dirp);
  } catch (const std::exception &ex) {
    closedir(dirp);
    throw;
  }

  return results;
}

DatapackStore::DatapackStore(const std::string &path) :
    _path(path),
    _lastRefresh(0) {
  // Find pack files in path
  std::vector<std::string> files = getAvailablePackFiles(path);

  for(std::vector<std::string>::iterator it = files.begin();
      it != files.end();
      it++) {

    std::string &packpath = *it;
    addPack(packpath);
  }
}

datapack_handle_t *DatapackStore::addPack(const std::string &path) {
  std::string idx_path(path + INDEXSUFFIX);
  std::string data_path(path + PACKSUFFIX);

  datapack_handle_t *pack = open_datapack(
    (char*)idx_path.c_str(), idx_path.size(),
    (char*)data_path.c_str(), data_path.size());
  if (pack == NULL) {
    return NULL;
  }

  if (pack->status == DATAPACK_HANDLE_OK) {
    _packs.push_back(pack);
    _packPaths.insert(path);
    return pack;
  } else {
    free(pack);
    return NULL;
  }
}

DatapackStore::~DatapackStore() {
  for(std::vector<datapack_handle_t*>::iterator it = _packs.begin();
      it != _packs.end();
      it++) {
    close_datapack(*it);
  }
}

DeltaChainIterator DatapackStore::getDeltaChain(const Key &key) {
  delta_chain_t chain = this->getDeltaChainRaw(key);
  if (chain.code == GET_DELTA_CHAIN_OK) {
    return DeltaChainIterator(chain);
  }

  freedeltachain(chain);

  throw MissingKeyError("unable to find delta chain");
}

delta_chain_t DatapackStore::getDeltaChainRaw(const Key &key) {
  for(std::vector<datapack_handle_t*>::iterator it = _packs.begin();
      it != _packs.end();
      it++) {
    datapack_handle_t *pack = *it;

    delta_chain_t chain = getdeltachain(pack, (const uint8_t *) key.node);
    if (chain.code == GET_DELTA_CHAIN_OOM) {
      throw std::runtime_error("out of memory");
    } else if (chain.code == GET_DELTA_CHAIN_NOT_FOUND) {
      freedeltachain(chain);
      continue;
    } else if (chain.code != GET_DELTA_CHAIN_OK) {
      freedeltachain(chain);
      continue;
    }

    return chain;
  }

  // Check if there are new packs available
  std::vector<datapack_handle_t*> refreshed = refresh();
  for(std::vector<datapack_handle_t*>::iterator it = refreshed.begin();
      it != refreshed.end();
      it++) {
    datapack_handle_t *pack = *it;

    delta_chain_t chain = getdeltachain(pack, (const uint8_t *) key.node);
    if (chain.code == GET_DELTA_CHAIN_OOM) {
      throw std::runtime_error("out of memory");
    } else if (chain.code == GET_DELTA_CHAIN_NOT_FOUND) {
      freedeltachain(chain);
      continue;
    } else if (chain.code != GET_DELTA_CHAIN_OK) {
      freedeltachain(chain);
      continue;
    }

    return chain;
  }

  return COMPOUND_LITERAL(delta_chain_t) { GET_DELTA_CHAIN_NOT_FOUND };
}

Key *DatapackStoreKeyIterator::next() {
  Key *key;
  while ((key = _missing.next()) != NULL) {
    if (!_store.contains(*key)) {
      return key;
    }
  }

  return NULL;
}

bool DatapackStore::contains(const Key &key) {
  for(std::vector<datapack_handle_t*>::iterator it = _packs.begin();
      it != _packs.end();
      it++) {
    datapack_handle_t *pack = *it;

    pack_index_entry_t packindex;
    if (find(pack, (uint8_t*)key.node, &packindex)) {
      return true;
    }
  }

  // Check if there are new packs available
  std::vector<datapack_handle_t*> refreshed = refresh();
  for(std::vector<datapack_handle_t*>::iterator it = refreshed.begin();
      it != refreshed.end();
      it++) {
    datapack_handle_t *pack = *it;

    pack_index_entry_t packindex;
    if (find(pack, (uint8_t*)key.node, &packindex)) {
      return true;
    }
  }

  return false;
}

DatapackStoreKeyIterator DatapackStore::getMissing(KeyIterator &missing) {
  return DatapackStoreKeyIterator(*this, missing);
}

std::vector<datapack_handle_t*> DatapackStore::refresh() {
  clock_t now = clock();

  std::vector<datapack_handle_t*> newPacks;
  if (now - _lastRefresh > PACK_REFRESH_RATE) {
    std::vector<std::string> availablePacks = getAvailablePackFiles(_path);
    for(std::vector<std::string>::iterator it = availablePacks.begin();
        it != availablePacks.end();
        it++) {
      std::string &packPath = *it;
      if (_packPaths.find(packPath) == _packPaths.end()) {
        datapack_handle_t *newPack = addPack(packPath);
        if (newPack) {
          newPacks.push_back(newPack);
        }
      }
    }

    _lastRefresh = now;
  }

  return newPacks;
}

void DatapackStore::markForRefresh() {
  _lastRefresh = 0;
}
