// manifest.h - c++ declarations for a single manifest
//
// Copyright 2016 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.
//
// no-check-code

#ifndef REMOTEFILELOG_MANIFEST_H
#define REMOTEFILELOG_MANIFEST_H

#include <list>

class Manifest;
class ManifestIterator;

#include "manifest_entry.h"
#include "pythonutil.h"

/**
 * This class represents a view on a particular Manifest instance. It provides
 * access to the list of files/directories at one level of the tree, not the
 * entire tree.
 *
 * Instances of this class do not own the actual storage of manifest data. This
 * class just provides a view onto that existing storage.
 *
 * If the actual manifest data comes from the store, this class refers to it via
 * a PythonObj, and reference counting is used to determine when it's cleaned
 * up.
 *
 * If the actual manifest data comes from an InMemoryManifest, then the life
 * time of that InMemoryManifest is managed elsewhere, and is unaffected by the
 * existence of Manifest objects that view into it.
 */
class Manifest {
  private:
    PythonObj _rawobj;

    std::list<ManifestEntry> entries;

  public:
    Manifest() {
    }

    Manifest(PythonObj &rawobj);

    ManifestIterator getIterator() const;
};

/**
 * Class that represents an iterator over the entries of an individual
 * manifest.
 */
class ManifestIterator {
  private:
    std::list<ManifestEntry>::const_iterator iterator;
    std::list<ManifestEntry>::const_iterator end;
  public:
    ManifestIterator() {
    }

    ManifestIterator(
        std::list<ManifestEntry>::const_iterator iterator,
        std::list<ManifestEntry>::const_iterator end);

    bool next(ManifestEntry *entry);

    ManifestEntry currentvalue() const;

    bool isfinished() const;
};

#endif //REMOTEFILELOG_MANIFEST_H
