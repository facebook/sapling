// treemanifest.h - c++ declarations of a tree manifest
//
// Copyright 2016 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.
//
// no-check-code

#ifndef REMOTEFILELOG_TREEMANIFEST_H
#define REMOTEFILELOG_TREEMANIFEST_H

#include <string>
#include <vector>

#include "manifest_fetcher.h"
#include "pythonutil.h"

/**
 * A single instance of a treemanifest.
 */
struct treemanifest {
  // A reference to the store that is used to fetch new content
  PythonObj store;

  // The 20-byte root node of this manifest
  std::string rootNode;

  // The resolved Manifest node, if the root has already been resolved.
  Manifest *rootManifest;

  treemanifest(PythonObj store, std::string rootNode) :
      store(store),
      rootNode(rootNode),
      rootManifest(NULL) {
  }
};

/**
 * Represents a single stack frame in an iteration of the contents of the tree.
 */
struct stackframe {
  Manifest *manifest;
  ManifestIterator iterator;

  stackframe(Manifest *manifest) :
      manifest(manifest),
      iterator(manifest->getIterator()) {
  }
};

/**
 * A helper struct representing the state of an iterator recursing over a tree.
 */
struct fileiter {
  ManifestFetcher fetcher;      // Instance to fetch tree content
  std::vector<stackframe> frames;
  std::string path;             // The fullpath for the top entry in the stack.

  fileiter(ManifestFetcher fetcher) :
      fetcher(fetcher) {
  }

  fileiter(const fileiter &old) :
      fetcher(old.fetcher),
      frames(old.frames),
      path(old.path) {
  }

  fileiter& operator=(const fileiter &other) {
    this->fetcher = other.fetcher;
    this->frames = other.frames;
    this->path = other.path;

    return *this;
  }
};

class PathIterator {
  private:
    std::string path;
    size_t position;
  public:
    PathIterator(std::string path) {
      this->path = path;
      this->position = 0;
    }

    bool next(char const ** word, size_t *wordlen) {
      if (this->isfinished()) {
        return false;
      }

      *word = this->path.c_str() + this->position;
      size_t slashoffset = this->path.find('/', this->position);
      if (slashoffset == std::string::npos) {
        *wordlen = this->path.length() - this->position;
      } else {
        *wordlen = slashoffset - this->position;
      }

      this->position += *wordlen + 1;

      return true;
    }

    bool isfinished() {
      return this->position >= this->path.length();
    }
};

extern void _treemanifest_find(
    const std::string &filename,
    const std::string &rootnode,
    Manifest **cachedlookup,
    const ManifestFetcher &fetcher,
    std::string *resultnode, char *resultflag);

#endif //REMOTEFILELOG_TREEMANIFEST_H
