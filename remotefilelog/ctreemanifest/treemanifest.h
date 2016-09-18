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

#include "pythonutil.h"

#include <string>
#include <vector>

#include "manifest_fetcher.h"

enum FindResult {
  FIND_PATH_OK,
  FIND_PATH_NOT_FOUND,
  FIND_PATH_CONFLICT,
  FIND_PATH_WTF,
};

enum SetResult {
  SET_OK,
  SET_CONFLICT,
  SET_WTF,
};

enum FindMode {
  // walks the tree and searches for a leaf node.  if the path cannot be found,
  // exit with `FIND_PATH_NOT_FOUND`.
      BASIC_WALK,

  // walks the tree.  if the intermediate paths cannot be found, create them.
  // if a leaf node exists where an intermediate path node needs to be
  // created, then return `FIND_PATH_CONFLICT`.
      CREATE_IF_MISSING,

  // walks the tree.  if the path cannot be found, exit with
  // `FIND_PATH_NOT_FOUND`.  if the operation is successful, then check
  // intermediate nodes to ensure that they still have children.  any nodes
  // that do not should be removed.
      REMOVE_EMPTY_IMPLICIT_NODES,
};

struct FindContext {
  bool invalidate_checksums;
  int32_t num_leaf_node_changes;
  FindMode mode;

  // reuse this space when fetching manifests.
  std::string nodebuffer;

  // any extra data the callback needs to complete the operation.
  void *extras;
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

    void getPathToPosition(const char **word, size_t *wordlen) {
      *word = path.c_str();
      *wordlen = this->position;
    }
};

/**
 * A single instance of a treemanifest.
 */
struct treemanifest {
    // Fetcher for the manifests.
    ManifestFetcher fetcher;

    ManifestEntry root;

    treemanifest(PythonObj store, std::string rootNode) :
        fetcher(store) {
      std::string hexnode;
      hexnode.reserve(HEX_NODE_SIZE);

      hexfrombin(rootNode.c_str(), hexnode);
      root.initialize(NULL, 0, hexnode.c_str(), MANIFEST_DIRECTORY_FLAGPTR);

      // ManifestEntry.initialize will create a blank manifest in .resolved.
      // however, we actually want the resolution to happen through
      // manifestfetcher.  therefore, let's delete the field and clear it.
      delete root.resolved;
      root.resolved = NULL;
    }

    treemanifest(ManifestFetcher fetcher, ManifestEntry *otherRoot) :
        fetcher(fetcher) {
      root.initialize(otherRoot);
    }

    treemanifest(PythonObj store) :
        fetcher(store) {
      std::string hexnode;
      hexnode.assign(HEX_NODE_SIZE, '\0');

      root.initialize(NULL, 0, hexnode.c_str(), MANIFEST_DIRECTORY_FLAGPTR);
    }

    treemanifest *copy();

    void get(
        const std::string &filename,
        std::string *resultnode, const char **resultflag);

    SetResult set(
        const std::string &filename,
        const std::string &resultnode, const char *resultflag);

    /**
     * Removes a file from the treemanifest.  Returns true iff the file was
     * found and removed.
     */
    bool remove(const std::string &filename);

    Manifest *getRootManifest() {
      if (this->root.resolved == NULL) {
        std::string binnode;
        binnode.reserve(BIN_NODE_SIZE);

        appendbinfromhex(this->root.node, binnode);
        this->root.resolved = this->fetcher.get("", 0, binnode);
      }

      return this->root.resolved;
    }

  private:

    /**
     * Basic mechanism to traverse a tree.  Once the deepest directory in the
     * path has been located, the supplied callback is executed.  That callback
     * is called with the manifest of the deepest directory and the leaf node's
     * filename.
     *
     * For instance, if treemanifest_find is called on /abc/def/ghi, then the
     * callback is executed with the manifest of /abc/def, and the filename
     * passed in will be "ghi".
     */
    FindResult find(
        ManifestEntry *manifestentry,
        PathIterator &path,
        FindMode findMode,
        FindContext *findContext,
        FindResult (*callback)(
            Manifest *manifest,
            const char *filename, size_t filenamelen,
            FindContext *findContext));
};

/**
 * Represents a single stack frame in an iteration of the contents of the tree.
 */
struct stackframe {
  private:
    ManifestIterator iterator;
    SortedManifestIterator sortedIterator;

  public:
    Manifest *manifest;
    bool sorted;

    stackframe(Manifest *manifest, bool sorted) :
        manifest(manifest),
        sorted(sorted) {
      if (sorted) {
        sortedIterator = manifest->getSortedIterator();
      } else {
        iterator = manifest->getIterator();
      }
    }

    ManifestEntry *next() {
      if (sorted) {
        return sortedIterator.next();
      } else {
        return iterator.next();
      }
    }

    ManifestEntry *currentvalue() const {
      if (sorted) {
        return sortedIterator.currentvalue();
      } else {
        return iterator.currentvalue();
      }
    }

    bool isfinished() const {
      if (sorted) {
        return sortedIterator.isfinished();
      } else {
        return iterator.isfinished();
      }
    }
};

/**
 * A helper struct representing the state of an iterator recursing over a tree.
 */
struct fileiter {
  ManifestFetcher fetcher;      // Instance to fetch tree content
  std::vector<stackframe> frames;
  std::string path;             // The fullpath for the top entry in the stack.
  bool sorted;                  // enable mercurial sorting?

  // If provided, the given matcher filters the results by path
  PythonObj matcher;

  fileiter(treemanifest &tm, bool sorted) :
      fetcher(tm.fetcher),
      sorted(sorted) {
    this->frames.push_back(stackframe(tm.getRootManifest(), this->sorted));
    this->path.reserve(1024);
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

extern void treemanifest_diffrecurse(
    Manifest *selfmf,
    Manifest *othermf,
    std::string &path,
    const PythonObj &diff,
    const ManifestFetcher &fetcher);

#endif //REMOTEFILELOG_TREEMANIFEST_H
