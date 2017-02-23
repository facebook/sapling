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
    _path(path) {
  // Find pack files in path
  std::vector<std::string> files = getAvailablePackFiles(path);

  for(std::vector<std::string>::iterator it = files.begin();
      it != files.end();
      it++) {

    std::string &packpath = *it;
    char idx_path[packpath.size() + INDEXSUFFIXLEN];
    char data_path[packpath.size() + PACKSUFFIXLEN];

    sprintf(idx_path, "%s%s", packpath.c_str(), INDEXSUFFIX);
    sprintf(data_path, "%s%s", packpath.c_str(), PACKSUFFIX);

    datapack_handle_t *pack = open_datapack(
      idx_path, strlen(idx_path),
      data_path, strlen(data_path));
    if (pack == NULL) {
      continue;
    }

    if (pack->status == DATAPACK_HANDLE_OK) {
      _packs.push_back(pack);
    } else {
      free(pack);
    }
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

    return DeltaChainIterator(chain);
  }

  throw MissingKeyError("unable to find delta chain");
}
