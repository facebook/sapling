# Infinite push
#
# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

class indexapi(object):
    def __init__(self):
        """Initializes the metadata store connection."""
        pass

    def close(self):
        """Cleans up the metadata store connection."""
        pass

    def addbundle(self, bundleid, nodes):
        """Takes a bundleid and a list of nodes in that bundle and records that
        each node is contained in that bundle."""
        raise NotImplementedError()

    def addbookmark(self, bookmark, node):
        """Takes a bookmark name and hash, and records mapping in the metadata
        store."""
        raise NotImplementedError()

    def addbookmarkandbundle(self, bundleid, nodes, bookmark, bookmarknode):
        """Atomic addbundle() + addbookmark()"""
        raise NotImplementedError()

    def deletebookmarks(self, patterns, commit):
        """Accepts list of bookmarks and deletes them.
        `commit` may not be implemented by some indexapis.
        The meaning of the `commit` parameter: if  it is set then bookmark
        will actually be deleted when deletebookmarks returns. Otherwise
        deletion will be delayed until the end of transaction.
        """
        raise NotImplementedError()

    def getbundle(self, node):
        """Returns the bundleid for the bundle that contains the given node."""
        raise NotImplementedError()

    def getnode(self, bookmark):
        """Returns the node for the given bookmark. None if it doesn't exist."""
        raise NotImplementedError()

    def getbookmarks(self, query):
        """Returns bookmarks that match the query"""
        raise NotImplementedError()

class indexexception(Exception):
    pass
