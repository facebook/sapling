# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# smallcommitmetadata.py - stores a small amount of metadata associated with a commit


from . import json
from .node import bin, hex
from .util import altsortdict


# Stores a mapping of (node, category) -> data, with a FIFO-limited number of entries
class smallcommitmetadata(object):
    def __init__(self, vfs, entrylimit):
        self.vfs = vfs
        self.limit = entrylimit
        self.contents = altsortdict()
        self.reload()

    def reload(self):
        """Read the database from disk."""
        if not self.vfs.exists("commit_metadata"):
            self.contents = altsortdict()
            return
        try:
            entries = json.loads(self.vfs.tryreadutf8("commit_metadata"))[-self.limit :]
        except ValueError:
            entries = []
        for entry in entries:
            self.contents[(bin(entry["node"]), entry["category"])] = entry["data"]

    def write(self):
        """Write the database to disk."""
        with self.vfs("commit_metadata", "w", atomictemp=True) as f:
            entries = [
                {"node": hex(node), "category": category, "data": data}
                for ((node, category), data) in self.contents.items()
            ]
            serialized = json.dumps(entries)
            f.write(serialized.encode("utf8"))

    def store(self, node, category, data):
        """Adds a new entry with the specified node and category, and updates the data on disk. Returns the removed entry, if any."""
        self.contents[(node, category)] = data
        popped = None
        while len(self.contents) > self.limit:
            popped = self.contents.popitem(last=False)
        self.write()
        return popped

    def delete(self, node, category):
        """Removes the entry with matching node and category and returns its value."""
        value = self.contents[(node, category)]
        del self.contents[(node, category)]
        return value

    def read(self, node, category):
        """Returns the value of the entry with specified node and category."""
        return self.contents[(node, category)]

    def find(self, node=None, category=None):
        """Returns a map of all entries with matching node and/or category. If both are None, returns all entries."""
        return altsortdict(
            (
                ((node_, category_), data)
                for ((node_, category_), data) in self.contents.items()
                if node is None or node == node_
                if category is None or category == category_
            )
        )

    def finddelete(self, node=None, category=None):
        """Removes and returns any entries with matching node and/or category."""
        entriestoremove = [
            ((node_, category_), data_)
            for ((node_, category_), data_) in self.contents.items()
            if node is None or node == node_
            if category is None or category == category_
        ]
        for (key, _value) in entriestoremove:
            del self.contents[key]
        return altsortdict(entriestoremove)

    def clear(self):
        """Removes and returns all entries."""
        deleted = self.contents
        self.contents = altsortdict()
        return deleted
