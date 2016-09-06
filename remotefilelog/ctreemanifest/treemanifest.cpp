// treemanifest.cpp - c++ implementation of a tree manifest
//
// Copyright 2016 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.
//
// no-check-code

#include "treemanifest.h"

treemanifest::~treemanifest() {
  if (this->rootManifest != NULL) {
    delete this->rootManifest;
  }
}

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
static FindResult treemanifest_find(
    Manifest *manifest,
    PathIterator &path,
    const ManifestFetcher &fetcher,
    FindMode findMode,
    FindContext *findContext,
    FindResult (*callback)(
        Manifest *manifest,
        const char *filename, size_t filenamelen,
        FindContext *findContext)) {

  const char *word;
  size_t wordlen;

  path.next(&word, &wordlen);
  if (path.isfinished()) {
    // time to execute the callback.
    return callback(manifest,
        word, wordlen,
        findContext);
  } else {
    // position the iterator at the right location
    bool exacthit;
    std::list<ManifestEntry>::iterator iterator = manifest->findChild(
        word, wordlen, &exacthit);

    ManifestEntry *entry;

    if (!exacthit) {
      // do we create the intermediate node?
      if (findMode != CREATE_IF_MISSING) {
        return FIND_PATH_NOT_FOUND;
      }

      // create the intermediate node...
      entry = manifest->addChild(iterator, word, wordlen, 't');
    } else {
      entry = &(*iterator);

      if (!entry->isdirectory()) {
        return FIND_PATH_CONFLICT;
      }

      if (entry->resolved == NULL) {
        const char *pathstart;
        size_t pathlen;

        path.getPathToPosition(&pathstart, &pathlen);
        findContext->nodebuffer.erase();
        appendbinfromhex(entry->node, findContext->nodebuffer);
        entry->resolved = fetcher.get(pathstart, pathlen,
            findContext->nodebuffer);
      }
    }

    // now find the next subdir
    FindResult result = treemanifest_find(
        entry->resolved,
        path,
        fetcher,
        findMode,
        findContext,
        callback);

    // if entry->resolved has 0 entries, we may want to prune it, if the mode
    // indicates that we should.
    if (findMode == REMOVE_EMPTY_IMPLICIT_NODES) {
      if (entry->resolved->children() == 0) {
        manifest->removeChild(iterator);
      }
    }

    return result;
  }
}

struct GetResult {
  std::string *resultnode;
  char *resultflag;
};

static FindResult treemanifest_get_callback(
    Manifest *manifest,
    const char *filename, size_t filenamelen,
    FindContext *context) {
  // position the iterator at the right location
  bool exacthit;
  std::list<ManifestEntry>::iterator iterator = manifest->findChild(
      filename, filenamelen, &exacthit);

  if (!exacthit) {
    // TODO: not found. :( :(
    return FIND_PATH_NOT_FOUND;
  }

  ManifestEntry &entry = *iterator;
  GetResult *result = (GetResult *) context->extras;

  result->resultnode->erase();
  if (entry.node != NULL) {
    result->resultnode->append(entry.node);
  }

  if (entry.flag != NULL) {
    *result->resultflag = *entry.flag;
  } else {
    *result->resultflag = '\0';
  }

  return FIND_PATH_OK;
}

void treemanifest_get(
    const std::string &filename,
    Manifest *rootmanifest,
    const ManifestFetcher &fetcher,
    std::string *resultnode, char *resultflag) {
  GetResult extras = {resultnode, resultflag};
  PathIterator pathiter(filename);
  FindContext changes;
  changes.nodebuffer.reserve(20);
  changes.extras = &extras;

  treemanifest_find(
      rootmanifest,
      pathiter,
      fetcher,
      BASIC_WALK,
      &changes,
      treemanifest_get_callback
  );
}
