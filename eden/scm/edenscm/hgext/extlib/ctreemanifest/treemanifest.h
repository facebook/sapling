// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// treemanifest.h - c++ declarations of a tree manifest
// no-check-code

#ifndef FBHGEXT_CTREEMANIFEST_TREEMANIFEST_H
#define FBHGEXT_CTREEMANIFEST_TREEMANIFEST_H

#include <memory>
#include <string>
#include <vector>

#include "edenscm/hgext/extlib/cstore/match.h"
#include "edenscm/hgext/extlib/ctreemanifest/manifest.h"
#include "edenscm/hgext/extlib/ctreemanifest/manifest_entry.h"
#include "edenscm/hgext/extlib/ctreemanifest/manifest_fetcher.h"

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
  void* extras;

  FindContext()
      : invalidate_checksums(false),
        num_leaf_node_changes(0),
        mode(BASIC_WALK),
        extras(NULL) {}
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

  bool next(char const** word, size_t* wordlen) {
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

  void getPathToPosition(const char** word, size_t* wordlen) {
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

  treemanifest(std::shared_ptr<Store> store, std::string rootNode)
      : fetcher(store) {
    std::string hexnode;
    hexnode.reserve(HEX_NODE_SIZE);

    hexfrombin(rootNode.c_str(), hexnode);
    root.initialize(NULL, 0, hexnode.c_str(), MANIFEST_DIRECTORY_FLAGPTR);

    // ManifestEntry.initialize will create a blank manifest in .resolved.
    // however, we actually want the resolution to happen through
    // manifestfetcher.  therefore, let's clear it.
    root.resolved = ManifestPtr();
  }

  treemanifest(std::shared_ptr<Store> store) : fetcher(store) {
    root.initialize(NULL, 0, HEXNULLID, MANIFEST_DIRECTORY_FLAGPTR);
  }

  treemanifest(treemanifest& other) : fetcher(other.fetcher) {
    root.initialize(&other.root);
  }

  bool get(
      const std::string& filename,
      std::string* resultnode,
      const char** resultflag,
      FindResultType resulttype = RESULT_FILE,
      ManifestPtr* resultmanifest = nullptr);

  SetResult set(
      const std::string& filename,
      const std::string& resultnode,
      const char* resultflag);

  /**
   * Removes a file from the treemanifest.  Returns true iff the file was
   * found and removed.
   */
  bool remove(const std::string& filename);

  ManifestPtr getRootManifest() {
    if (this->root.resolved.isnull()) {
      std::string binnode;
      binnode.reserve(BIN_NODE_SIZE);

      appendbinfromhex(this->root.get_node(), binnode);
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
      ManifestEntry* manifestentry,
      PathIterator& path,
      FindMode findMode,
      FindContext* findContext,
      FindResult (*callback)(
          Manifest* manifest,
          const char* filename,
          size_t filenamelen,
          FindContext* findContext,
          ManifestPtr* resultManifest),
      ManifestPtr* resultManifest);
};

/**
 * Represents a single stack frame in an iteration of the contents of the tree.
 */
struct stackframe {
 private:
  ManifestIterator iterator;
  SortedManifestIterator sortedIterator;

 public:
  ManifestPtr manifest;
  bool sorted;

  stackframe(ManifestPtr manifest, bool sorted)
      : manifest(manifest), sorted(sorted) {
    if (sorted) {
      sortedIterator = manifest->getSortedIterator();
    } else {
      iterator = manifest->getIterator();
    }
  }

  ManifestEntry* next() {
    if (sorted) {
      return sortedIterator.next();
    } else {
      return iterator.next();
    }
  }

  ManifestEntry* currentvalue() const {
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
 * An iterator that takes a main treemanifest and a vector of comparison
 * treemanifests and iterates over the Manifests that only exist in the main
 * treemanifest.
 */
class SubtreeIterator {
 private:
  std::vector<stackframe> mainStack;
  std::vector<const char*> cmpNodes;
  std::vector<std::vector<stackframe>> cmpStacks;
  std::string path;
  ManifestFetcher fetcher;
  bool firstRun;
  int maxDepth;
  int depth;

 public:
  SubtreeIterator(
      std::string path,
      ManifestPtr mainRoot,
      const std::vector<const char*>& cmpNodes,
      const std::vector<ManifestPtr>& cmpRoots,
      const ManifestFetcher& fetcher,
      const int depth);

  /**
   * Outputs the next new Manifest and its corresponding path and node.
   *
   * `resultEntry` contains the ManifestEntry that points at the result. This
   * is useful for updating the ManifestEntry hash if the caller decides to
   * make the result permanent.
   *
   * Return true if a manifest was returned, or false if we've reached the
   * end.
   */
  bool next(
      std::string** path,
      ManifestPtr* result,
      ManifestPtr* p1,
      ManifestPtr* p2);

 private:
  /**
   * Pops the current Manifest, populating the output values and returning true
   * if the current Manifest is different from all comparison manifests.
   */
  void popResult(
      std::string** path,
      ManifestPtr* result,
      ManifestPtr* p1,
      ManifestPtr* p2);

  /** Pushes the given Manifest onto the stacks. If the given Manifest equals
   * one of the comparison Manifests, the function does nothing.
   */
  bool processDirectory(ManifestEntry* mainEntry);
};

class FinalizeIterator {
 private:
  SubtreeIterator _iterator;

 public:
  FinalizeIterator(
      ManifestPtr mainRoot,
      const std::vector<const char*>& cmpNodes,
      const std::vector<ManifestPtr>& cmpRoots,
      const ManifestFetcher& fetcher);

  bool next(
      std::string** path,
      ManifestPtr* result,
      ManifestPtr* p1,
      ManifestPtr* p2);
};

/**
 * A helper struct representing the state of an iterator recursing over a tree.
 */
struct fileiter {
  ManifestFetcher fetcher; // Instance to fetch tree content
  std::vector<stackframe> frames;
  std::string path; // The fullpath for the top entry in the stack.
  bool sorted; // enable mercurial sorting?

  // If provided, the given matcher filters the results by path
  std::shared_ptr<Matcher> matcher;

  fileiter(treemanifest& tm, bool sorted)
      : fetcher(tm.fetcher), sorted(sorted) {
    this->frames.push_back(stackframe(tm.getRootManifest(), this->sorted));
    this->path.reserve(1024);
  }

  fileiter(const fileiter& old)
      : fetcher(old.fetcher), frames(old.frames), path(old.path) {}

  fileiter& operator=(const fileiter& other) {
    this->fetcher = other.fetcher;
    this->frames = other.frames;
    this->path = other.path;

    return *this;
  }
};

struct DiffResult {
  virtual ~DiffResult() {}
  virtual void add(
      const std::string& path,
      const char* beforeNode,
      const char* beforeFlag,
      const char* afterNode,
      const char* afterFlag) = 0;
  virtual void addclean(const std::string& path) = 0;
};

extern void treemanifest_diffrecurse(
    Manifest* selfmf,
    Manifest* othermf,
    std::string& path,
    DiffResult& diff,
    const ManifestFetcher& fetcher,
    bool clean,
    Matcher& matcher);

#endif // FBHGEXT_CTREEMANIFEST_TREEMANIFEST_H
