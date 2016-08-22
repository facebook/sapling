// treemanifest.cpp - c++ implementation of a tree manifest
//
// Copyright 2016 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.
//
// no-check-code

#include "treemanifest.h"

void _treemanifest_find(
    const std::string &filename,
    const std::string &rootnode,
    const ManifestFetcher &fetcher,
    std::string *resultnode, char *resultflag) {
  size_t curpathlen = 0;
  std::string curnode(rootnode);

  // Loop over the parts of the query filename
  PathIterator pathiter(filename);
  const char *word;
  size_t wordlen;
  while (pathiter.next(&word, &wordlen)) {
    // Obtain the raw data for this directory
    Manifest *manifest = fetcher.get(filename.c_str(), curpathlen, curnode);

    // TODO: need to attach this manifest to the parent Manifest object.

    ManifestIterator mfiterator = manifest->getIterator();
    ManifestEntry *entry;
    bool recurse = false;

    // Loop over the contents of the current directory looking for the
    // next directory/file.
    while (mfiterator.next(&entry)) {
      // If the current entry matches the query file/directory, either recurse,
      // return, or abort.
      if (wordlen == entry->filenamelen &&
          strncmp(word, entry->filename, wordlen) == 0) {
        // If this is the last entry in the query path, either return or abort
        if (pathiter.isfinished()) {
          // If it's a file, it's our result
          if (!entry->isdirectory()) {
            resultnode->assign(binfromhex(entry->node));
            if (entry->flag == NULL) {
              *resultflag = '\0';
            } else {
              *resultflag = *entry->flag;
            }
            return;
          } else {
            // Found a directory when expecting a file - give up
            break;
          }
        }

        // If there's more in the query, either recurse or give up
        curpathlen = curpathlen + wordlen + 1;
        if (entry->isdirectory() && filename.length() > curpathlen) {
          curnode.erase();
          curnode.append(binfromhex(entry->node));
          recurse = true;
          break;
        } else {
          // Found a file when we expected a directory or
          // found a directory when we expected a file.
          break;
        }
      }
    }

    if (!recurse) {
      // Failed to find a match
      return;
    }
  }
}

