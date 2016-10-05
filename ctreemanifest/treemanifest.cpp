// treemanifest.cpp - c++ implementation of a tree manifest
//
// Copyright 2016 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.
//
// no-check-code

#include "treemanifest.h"

/**
 * Constructs a result python tuple of the given diff data.
 */
static PythonObj treemanifest_diffentry(
    const std::string *anode, const char *aflag,
    const std::string *bnode, const char *bflag) {
  const char *astr = anode != NULL ? anode->c_str() : NULL;
  Py_ssize_t alen = anode != NULL ? anode->length() : 0;
  const char *bstr = bnode != NULL ? bnode->c_str() : NULL;
  Py_ssize_t blen = bnode != NULL ? bnode->length() : 0;
  PythonObj result = Py_BuildValue(
      "((s#s#)(s#s#))",
      astr, alen,
      (aflag == NULL) ? MAGIC_EMPTY_STRING : aflag, Py_ssize_t(aflag ? 1 : 0),
      bstr, blen,
      (bflag == NULL) ? MAGIC_EMPTY_STRING : bflag, Py_ssize_t(bflag ? 1 : 0));
  return result;
}

/**
 * Simple class for representing a single diff between two files in the
 * manifest.
 */
class DiffEntry {
  private:
    const std::string *selfnode;
    const std::string *othernode;
    const char *selfflag;
    const char *otherflag;
  public:
    DiffEntry(const std::string *selfnode, const char *selfflag,
        const std::string *othernode, const char *otherflag) {
      this->selfnode = selfnode;
      this->othernode = othernode;
      this->selfflag = selfflag;
      this->otherflag = otherflag;
    }

    void addtodiff(const PythonObj &diff, const std::string &path) {
      PythonObj entry = treemanifest_diffentry(this->selfnode, this->selfflag,
          this->othernode, this->otherflag);
      PythonObj pathObj = PyString_FromStringAndSize(path.c_str(), path.length());

      PyDict_SetItem(diff, pathObj, entry);
    }
};

/**
 * Helper function that performs the actual recursion on the tree entries.
 */
void treemanifest_diffrecurse(
    Manifest *selfmf,
    Manifest *othermf,
    std::string &path,
    const PythonObj &diff,
    const ManifestFetcher &fetcher) {
  ManifestIterator selfiter;
  ManifestIterator otheriter;

  if (selfmf != NULL) {
    selfiter = selfmf->getIterator();
  }
  if (othermf != NULL) {
    otheriter = othermf->getIterator();
  }

  // Iterate through both directory contents
  while (!selfiter.isfinished() || !otheriter.isfinished()) {
    int cmp = 0;

    ManifestEntry *selfentry = NULL;
    std::string selfbinnode;
    if (!selfiter.isfinished()) {
      cmp--;
      selfentry = selfiter.currentvalue();
      selfbinnode = binfromhex(selfentry->node);
    }

    ManifestEntry *otherentry = NULL;
    std::string otherbinnode;
    if (!otheriter.isfinished()) {
      cmp++;
      otherentry = otheriter.currentvalue();
      otherbinnode = binfromhex(otherentry->node);
    }

    // If both sides are present, cmp == 0, so do a filename comparison
    if (cmp == 0) {
      cmp = strcmp(selfentry->filename, otherentry->filename);
    }

    int originalpathsize = path.size();
    if (cmp < 0) {
      // selfentry should be processed first and only exists in self
      selfentry->appendtopath(path);
      if (selfentry->isdirectory()) {
        Manifest *selfchildmanifest = selfentry->get_manifest(
            fetcher, path.c_str(), path.size());
        treemanifest_diffrecurse(selfchildmanifest, NULL, path, diff, fetcher);
      } else {
        DiffEntry entry(&selfbinnode, selfentry->flag, NULL, NULL);
        entry.addtodiff(diff, path);
      }
      selfiter.next();
    } else if (cmp > 0) {
      // otherentry should be processed first and only exists in other
      otherentry->appendtopath(path);
      if (otherentry->isdirectory()) {
        Manifest *otherchildmanifest = otherentry->get_manifest(
            fetcher, path.c_str(), path.size());
        treemanifest_diffrecurse(NULL, otherchildmanifest, path, diff, fetcher);
      } else {
        DiffEntry entry(NULL, NULL, &otherbinnode, otherentry->flag);
        entry.addtodiff(diff, path);
      }
      otheriter.next();
    } else {
      // Filenames match - now compare directory vs file
      if (selfentry->isdirectory() && otherentry->isdirectory()) {
        // Both are directories - recurse
        selfentry->appendtopath(path);

        if (selfbinnode != otherbinnode) {
          Manifest *selfchildmanifest = selfentry->get_manifest(fetcher,
              path.c_str(), path.size());
          Manifest *otherchildmanifest = otherentry->get_manifest(fetcher,
              path.c_str(), path.size());

          treemanifest_diffrecurse(
              selfchildmanifest,
              otherchildmanifest,
              path,
              diff,
              fetcher);
        }
        selfiter.next();
        otheriter.next();
      } else if (selfentry->isdirectory() && !otherentry->isdirectory()) {
        // self is directory, other is not - process other then self
        otherentry->appendtopath(path);
        DiffEntry entry(NULL, NULL, &otherbinnode, otherentry->flag);
        entry.addtodiff(diff, path);

        path.append(1, '/');
        Manifest *selfchildmanifest = selfentry->get_manifest(fetcher,
            path.c_str(), path.size());
        treemanifest_diffrecurse(selfchildmanifest, NULL, path, diff, fetcher);

        selfiter.next();
        otheriter.next();
      } else if (!selfentry->isdirectory() && otherentry->isdirectory()) {
        // self is not directory, other is - process self then other
        selfentry->appendtopath(path);
        DiffEntry entry(&selfbinnode, selfentry->flag, NULL, NULL);
        entry.addtodiff(diff, path);

        path.append(1, '/');
        Manifest *otherchildmanifest = otherentry->get_manifest(fetcher,
            path.c_str(), path.size()
        );
        treemanifest_diffrecurse(NULL, otherchildmanifest, path, diff, fetcher);

        selfiter.next();
        otheriter.next();
      } else {
        // both are files
        bool flagsdiffer = (
            (selfentry->flag && otherentry->flag && *selfentry->flag != *otherentry->flag) ||
            ((bool)selfentry->flag != (bool)otherentry->flag)
        );

        if (selfbinnode != otherbinnode || flagsdiffer) {
          selfentry->appendtopath(path);
          DiffEntry entry(&selfbinnode, selfentry->flag, &otherbinnode, otherentry->flag);
          entry.addtodiff(diff, path);
        }

        selfiter.next();
        otheriter.next();
      }
    }
    path.erase(originalpathsize);
  }
}

FindResult treemanifest::find(
    ManifestEntry *manifestentry,
    PathIterator &path,
    FindMode findMode,
    FindContext *findContext,
    FindResult (*callback)(
        Manifest *manifest,
        const char *filename, size_t filenamelen,
        FindContext *findContext)) {
  if (manifestentry->resolved == NULL) {
    const char *pathstart;
    size_t pathlen;

    path.getPathToPosition(&pathstart, &pathlen);
    findContext->nodebuffer.erase();
    appendbinfromhex(manifestentry->node, findContext->nodebuffer);
    manifestentry->resolved = this->fetcher.get(pathstart, pathlen,
        findContext->nodebuffer);
  }
  Manifest *manifest = manifestentry->resolved;

  FindResult result;

  const char *word = NULL;
  size_t wordlen = 0;

  path.next(&word, &wordlen);
  if (path.isfinished()) {
    // time to execute the callback.
    result = callback(manifest,
        word, wordlen,
        findContext);
  } else {
    // position the iterator at the right location
    bool exacthit;
    std::list<ManifestEntry>::iterator iterator = manifest->findChild(
        word, wordlen, true, &exacthit);

    ManifestEntry *entry;

    if (!exacthit) {
      // do we create the intermediate node?
      if (findMode != CREATE_IF_MISSING) {
        return FIND_PATH_NOT_FOUND;
      }

      // create the intermediate node...
      entry = manifest->addChild(
          iterator, word, wordlen, NULL, MANIFEST_DIRECTORY_FLAGPTR);
    } else {
      entry = &(*iterator);
    }

    // now find the next subdir
    result = find(
        entry,
        path,
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
  }

  if (findContext->invalidate_checksums) {
    manifestentry->node = NULL;
  }

  return result;
}

struct GetResult {
  std::string *resultnode;
  const char **resultflag;
};

static FindResult get_callback(
    Manifest *manifest,
    const char *filename, size_t filenamelen,
    FindContext *context) {
  // position the iterator at the right location
  bool exacthit;
  std::list<ManifestEntry>::iterator iterator = manifest->findChild(
      filename, filenamelen, false, &exacthit);

  if (!exacthit) {
    // TODO: not found. :( :(
    return FIND_PATH_NOT_FOUND;
  }

  ManifestEntry &entry = *iterator;
  GetResult *result = (GetResult *) context->extras;

  result->resultnode->erase();
  if (entry.node != NULL) {
    appendbinfromhex(entry.node, *result->resultnode);
  }

  *result->resultflag = entry.flag;

  return FIND_PATH_OK;
}

void treemanifest::get(
    const std::string &filename,
    std::string *resultnode, const char **resultflag) {
  getRootManifest();

  GetResult extras = {resultnode, resultflag};
  PathIterator pathiter(filename);
  FindContext changes;
  changes.nodebuffer.reserve(BIN_NODE_SIZE);
  changes.extras = &extras;

  this->find(
      &this->root,
      pathiter,
      BASIC_WALK,
      &changes,
      get_callback
  );
}

struct SetParams {
  const std::string &resultnode;
  const char *resultflag;
};

static FindResult set_callback(
    Manifest *manifest,
    const char *filename, size_t filenamelen,
    FindContext *context) {
  SetParams *params = (SetParams *) context->extras;

  // position the iterator at the right location
  bool exacthit;
  std::list<ManifestEntry>::iterator iterator = manifest->findChild(
      filename, filenamelen, false, &exacthit);

  if (!exacthit) {
    // create the entry, insert it.
    manifest->addChild(
        iterator,
        filename, filenamelen,
        params->resultnode.c_str(), params->resultflag);
  } else {
    ManifestEntry *entry = &(*iterator);

    entry->update(params->resultnode.c_str(), params->resultflag);
  }
  context->invalidate_checksums = true;

  return FIND_PATH_OK;
}

SetResult treemanifest::set(
    const std::string &filename,
    const std::string &resultnode,
    const char *resultflag) {
  SetParams extras = {resultnode, resultflag};
  PathIterator pathiter(filename);
  FindContext changes;
  changes.nodebuffer.reserve(BIN_NODE_SIZE);
  changes.extras = &extras;

  FindResult result = this->find(
      &this->root,
      pathiter,
      CREATE_IF_MISSING,
      &changes,
      set_callback
  );

  switch (result) {
    case FIND_PATH_OK:
      return SET_OK;
    case FIND_PATH_CONFLICT:
      return SET_CONFLICT;
    default:
      return SET_WTF;
  }
}

struct RemoveResult {
  bool found;
};

static FindResult remove_callback(
    Manifest *manifest,
    const char *filename, size_t filenamelen,
    FindContext *context) {
  RemoveResult *params = (RemoveResult *) context->extras;

  // position the iterator at the right location
  bool exacthit;
  std::list<ManifestEntry>::iterator iterator = manifest->findChild(
      filename, filenamelen, false, &exacthit);

  if (exacthit) {
    manifest->removeChild(iterator);
    params->found = true;
    context->invalidate_checksums = true;
  }

  return FIND_PATH_OK;
}

bool treemanifest::remove(
    const std::string &filename) {
  RemoveResult extras = {false};
  PathIterator pathiter(filename);
  FindContext changes;
  changes.nodebuffer.reserve(BIN_NODE_SIZE);
  changes.extras = &extras;

  FindResult result = this->find(
      &this->root,
      pathiter,
      REMOVE_EMPTY_IMPLICIT_NODES,
      &changes,
      remove_callback
  );

  return (result == FIND_PATH_OK) && extras.found;
}

NewTreeIterator::NewTreeIterator(Manifest *mainRoot,
                const std::vector<char*> &cmpNodes,
                const std::vector<Manifest*> &cmpRoots,
                const ManifestFetcher &fetcher) :
    cmpNodes(cmpNodes),
    fetcher(fetcher) {
  (void)(this->mainRoot);
  this->mainStack.push_back(stackframe(mainRoot, false));

  for (size_t i = 0; i < cmpRoots.size(); i++) {
    Manifest *cmpRoot = cmpRoots[i];

    std::vector<stackframe> stack;
    stack.push_back(stackframe(cmpRoot, false));
    this->cmpStacks.push_back(stack);
  }
}

bool NewTreeIterator::popResult(std::string **path, Manifest **result, std::string **node) {
  stackframe &mainFrame = this->mainStack.back();
  Manifest *mainManifest = mainFrame.manifest;
  std::string mainSerialized;

  // When we loop over the cmpStacks, record the cmp nodes that are parents
  // of the level we're about to return.
  char parentNodes[2][BIN_NODE_SIZE];
  memcpy(parentNodes[0], NULLID, BIN_NODE_SIZE);
  memcpy(parentNodes[1], NULLID, BIN_NODE_SIZE);

  bool alreadyExists = false;

  // Record the nodes of all cmp manifest equivalents
  for (size_t i = 0; i < cmpStacks.size(); i++) {
    // If a cmpstack is at the same level as the main stack, it represents
    // the same diretory and should be inspected.
    if (this->mainStack.size() == cmpStacks[i].size()) {
      std::vector<stackframe> &cmpStack = cmpStacks[i];
      Manifest *cmpManifest = cmpStack.back().manifest;

      if (!alreadyExists) {
        std::string cmpSerialized;
        cmpManifest->serialize(cmpSerialized);
        mainManifest->serialize(mainSerialized);

        // If the main manifest content is identical to a cmp content, we
        // shouldn't return it. Note: We already do this check when pushing
        // directories onto the stack, but for in-memory manifests we don't
        // know the node until after we've traversed the children, so we can't
        // verify their content until now.
        if (cmpSerialized.compare(mainSerialized) == 0) {
          alreadyExists = true;
        }
      }

      // Record the cmp parent nodes so later we can compute the main node
      if (cmpStack.size() > 1) {
        stackframe &priorCmpFrame = cmpStack[cmpStack.size() - 2];
        ManifestEntry *priorCmpEntry = priorCmpFrame.currentvalue();
        memcpy(parentNodes[i], binfromhex(priorCmpEntry->node).c_str(), BIN_NODE_SIZE);
      } else {
        // Use the original passed in parent nodes
        memcpy(parentNodes[i], binfromhex(this->cmpNodes[i]).c_str(), BIN_NODE_SIZE);
      }
    }
  }

  // We've finished processing this frame, so pop all the stacks
  this->mainStack.pop_back();
  for (size_t i = 0; i < cmpStacks.size(); i++) {
    if (this->mainStack.size() < cmpStacks[i].size()) {
      cmpStacks[i].pop_back();
    }
  }

  // If the current manifest has the same contents as a cmp manifest,
  // just give up now. Unless we're the root node (because the root node
  // will always change based on the parent nodes).
  if (alreadyExists && this->mainStack.size() > 1) {
    assert(this->node != NULL);
    return false;
  }

  // Update the node on the manifest entry
  char tempnode[BIN_NODE_SIZE];
  mainManifest->computeNode(parentNodes[0], parentNodes[1], tempnode);
  this->node.assign(tempnode, 20);
  if (mainStack.size() > 0) {
    // Peek back up the stack so we can put the right node on the
    // ManifestEntry.
    stackframe &priorFrame = mainStack[mainStack.size() - 1];
    ManifestEntry *priorEntry = priorFrame.currentvalue();

    std::string hexnode;
    hexfrombin(tempnode, hexnode);
    priorEntry->update(hexnode.c_str(), MANIFEST_DIRECTORY_FLAGPTR);
  }

  *path = &this->path;
  *result = mainManifest;
  *node = &this->node;
  return true;
}

bool NewTreeIterator::processDirectory(ManifestEntry *mainEntry) {
  // mainEntry is a new entry we need to compare against each cmpEntry, and
  // then push if it is different from all of them.

  // First move all the cmp iterators forward to the same name as mainEntry.
  bool alreadyExists = false;
  std::vector<std::vector<stackframe>*> requirePush;
  for (size_t i = 0; i < cmpStacks.size(); i++) {
    std::vector<stackframe> &cmpStack = cmpStacks[i];

    // If the cmpStack is at a different level, it is not at the same
    // location as main, so don't bother searching it.
    if (cmpStack.size() < mainStack.size()) {
      continue;
    }

    stackframe &cmpFrame = cmpStack.back();

    // Move cmp iterator forward until we match or pass the current
    // mainEntry filename.
    while (!cmpFrame.isfinished()) {
      ManifestEntry *cmpEntry = cmpFrame.currentvalue();
      int cmp = ManifestEntry::compareName(cmpEntry, mainEntry);
      if (cmp >= 0) {
        // If the directory names match...
        if (cmp == 0) {
          // And the nodes match...
          if (!alreadyExists &&
              (mainEntry->node && strncmp(mainEntry->node, cmpEntry->node, 40) == 0)) {
            // Skip this entry
            alreadyExists = true;
          }
          // Remember this stack so we can push to it later
          requirePush.push_back(&cmpStack);
        }
        break;
      }
      cmpFrame.next();
    }
  }

  // If mainEntry matched any of the cmpEntries, we should skip mainEntry.
  if (alreadyExists) {
    assert(mainEntry->node != NULL);
    return false;
  }

  // Otherwise, push to the main stack
  mainEntry->appendtopath(this->path);
  Manifest *mainManifest = mainEntry->get_manifest(this->fetcher,
      this->path.c_str(), this->path.size());
  this->mainStack.push_back(stackframe(mainManifest, false));

  // And push all cmp stacks we remembered that have the same directory.
  for (size_t i = 0; i < requirePush.size(); i++) {
    std::vector<stackframe> *cmpStack = requirePush[i];
    ManifestEntry *cmpEntry = cmpStack->back().currentvalue();
    Manifest *cmpManifest = cmpEntry->get_manifest(this->fetcher,
        this->path.c_str(), this->path.size());
    cmpStack->push_back(stackframe(cmpManifest, false));
  }

  return true;
}

bool NewTreeIterator::next(std::string **path, Manifest **result, std::string **node) {
  // Pop the last returned directory off the path
  size_t slashoffset = this->path.find_last_of('/', this->path.size() - 2);
  if (slashoffset == std::string::npos) {
    this->path.erase();
  } else {
    this->path.erase(slashoffset + 1);
  }

  while (true) {
    if (this->mainStack.empty()) {
      return false;
    }

    stackframe &mainFrame = this->mainStack.back();

    // If we've reached the end of this manifest, we've processed all the
    // children, so we can now return it.
    if (mainFrame.isfinished()) {
      // This can return false if this manifest ended up being equivalent to
      // a cmp parent manifest, which means we should skip it.
      if (this->popResult(path, result, node)) {
        if (this->mainStack.size() > 0) {
          this->mainStack.back().next();
        }
        return true;
      }
      if (this->mainStack.size() > 0) {
        this->mainStack.back().next();
      }
    } else {
      // Use currentvalue instead of next so that the stack of frames match the
      // actual current filepath.
      ManifestEntry *mainEntry = mainFrame.currentvalue();
      if (mainEntry->isdirectory()) {
        // If we're at a directory, process it, either by pushing it on the
        // stack, or by skipping it if it already matches a cmp parent.
        if (!this->processDirectory(mainEntry)) {
          mainFrame.next();
        }
      } else {
        mainFrame.next();
      }
    }
  }
}
