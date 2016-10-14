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

#include "pythonutil.h"

#include <list>
#include <stdexcept>
#include <openssl/sha.h>

class Manifest;
class ManifestIterator;
class SortedManifestIterator;

class ManifestPtr {
  private:
    Manifest *manifest;
  public:
    ManifestPtr();

    ManifestPtr(Manifest *manifest);

    ManifestPtr(const ManifestPtr &other);

    ~ManifestPtr();

    ManifestPtr& operator= (const ManifestPtr& other);

    operator Manifest* () const;

    Manifest *operator-> ();

    bool isnull() const;
};

#include "manifest_entry.h"

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
    size_t _refcount;

    std::list<ManifestEntry> entries;
    std::list<ManifestEntry *> mercurialSortedEntries;

  public:
    Manifest() :
      _refcount(0) {
    }

    Manifest(PythonObj &rawobj);

    void incref();
    size_t decref();

    /**
     * Returns a deep copy of this Manifest.
     */
    ManifestPtr copy();

    ManifestIterator getIterator();

    SortedManifestIterator getSortedIterator();

    /**
     * Returns an iterator correctly positioned for a child of a given
     * filename and directory/file status.  If a child with the same name
     * and directory/file status already exists, *exacthit will be set to
     * true.  Otherwise, it will be set to false.
     */
    std::list<ManifestEntry>::iterator findChild(
        const char *filename, const size_t filenamelen, bool isdir,
        bool *exacthit);

    /**
     * Adds a child with a given name.
     * @param iterator iterator for this->entries, correctly positioned for
     *                 the child.
     * @param filename
     * @param filenamelen
     */
    ManifestEntry *addChild(std::list<ManifestEntry>::iterator iterator,
        const char *filename, const size_t filenamelen, const char *node,
        const char *flag);

    /**
     * Adds a deep copy of the given ManifestEntry as a child.
     */
    ManifestEntry *addChild(std::list<ManifestEntry>::iterator iterator,
        ManifestEntry *otherChild);

    int children() const {
      return entries.size();
    }

    /**
     * Removes a child with referenced by the iterator.
     * @param iterator iterator for this->entries, correctly positioned for
     *                 the child.
     */
    void removeChild(
        std::list<ManifestEntry>::iterator iterator) {
      this->entries.erase(iterator);

      // invalidate the mercurial-ordered list of entries
      this->mercurialSortedEntries.clear();
    }

    /**
     * Computes the hash of this manifest, given the two parent nodes. The input
     * and output nodes are 20 bytes.
     */
    void computeNode(const char *p1, const char *p2, char *result);

    /**
     * Serializes the current manifest into the given string. The serialization
     * format matches upstream Mercurial's Manifest format and is appropriate
     * for putting in a store.
     */
    void serialize(std::string &result);
};

/**
 * Class that represents an iterator over the entries of an individual
 * manifest.
 */
class ManifestIterator {
  private:
    std::list<ManifestEntry>::iterator iterator;
    std::list<ManifestEntry>::const_iterator end;
  public:
    ManifestIterator() {
    }

    ManifestIterator(
        std::list<ManifestEntry>::iterator iterator,
        std::list<ManifestEntry>::const_iterator end);

    ManifestEntry *next();

    ManifestEntry *currentvalue() const;

    bool isfinished() const;
};

/**
 * Class that represents an iterator over the entries of an individual
 * manifest, sorted by mercurial's ordering.
 */
class SortedManifestIterator {
  private:
    std::list<ManifestEntry *>::iterator iterator;
    std::list<ManifestEntry *>::const_iterator end;
  public:
    SortedManifestIterator() {
    }

    SortedManifestIterator(
        std::list<ManifestEntry *>::iterator iterator,
        std::list<ManifestEntry *>::const_iterator end);

    ManifestEntry *next();

    ManifestEntry *currentvalue() const;

    bool isfinished() const;
};

#endif //REMOTEFILELOG_MANIFEST_H
