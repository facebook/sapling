import os
import time
import logging

import warnings
import mysql.connector

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

class sqlindexapi(indexapi):
    '''
    Sql backend for infinitepush index. See tables.

    CREATE TABLE IF NOT EXISTS nodestobundle(
    node CHAR(40) BINARY NOT NULL,
    bundle VARCHAR(512) BINARY NOT NULL,
    reponame CHAR(255) BINARY NOT NULL,
    PRIMARY KEY(node, reponame));

    CREATE TABLE IF NOT EXISTS bookmarkstonode(
    node CHAR(40) BINARY NOT NULL,
    bookmark VARCHAR(512) BINARY NOT NULL,
    reponame CHAR(255) BINARY NOT NULL,
    PRIMARY KEY(reponame, bookmark));

    CREATE TABLE IF NOT EXISTS bundles(
    bundle VARCHAR(512) BINARY NOT NULL,
    reponame CHAR(255) BINARY NOT NULL,
    PRIMARY KEY(bundle, reponame));
    '''

    def __init__(self, reponame, host, port,
                 database, user, password, logfile, loglevel):
        super(sqlindexapi, self).__init__()
        self.reponame = reponame
        self.sqlargs = {
            'host': host,
            'port': port,
            'database': database,
            'user': user,
            'password': password,
        }
        self.sqlconn = None
        self.sqlcursor = None
        if not logfile:
            logfile = os.devnull
        logging.basicConfig(filename=logfile)
        self.log = logging.getLogger()
        self.log.setLevel(loglevel)
        self._connected = False

    def sqlconnect(self):
        if self.sqlconn:
            raise indexexception("SQL connection already open")
        if self.sqlcursor:
            raise indexexception("SQL cursor already open without connection")
        retry = 3
        while True:
            try:
                self.sqlconn = mysql.connector.connect(
                    force_ipv6=True, **self.sqlargs)

                # Code is copy-pasted from hgsql. Bug fixes need to be
                # back-ported!
                # The default behavior is to return byte arrays, when we
                # need strings. This custom convert returns strings.
                self.sqlconn.set_converter_class(CustomConverter)
                self.sqlconn.autocommit = False
                break
            except mysql.connector.errors.Error:
                # mysql can be flakey occasionally, so do some minimal
                # retrying.
                retry -= 1
                if retry == 0:
                    raise
                time.sleep(0.2)

        waittimeout = 300
        waittimeout = self.sqlconn.converter.escape('%s' % waittimeout)

        self.sqlcursor = self.sqlconn.cursor()
        self._connected = True

    def close(self):
        """Cleans up the metadata store connection."""
        with warnings.catch_warnings():
            warnings.simplefilter("ignore")
            self.sqlcursor.close()
            self.sqlconn.close()
        self.sqlcursor = None
        self.sqlconn = None

    def addbundle(self, bundleid, nodes, commit=True):
        """Takes a bundleid and a list of nodes in that bundle and records that
        each node is contained in that bundle."""
        if not self._connected:
            self.sqlconnect()
        self.log.info("ADD BUNDLE %r %r %r" % (self.reponame, bundleid, nodes))
        self.sqlcursor.execute(
            "INSERT INTO bundles(bundle, reponame) VALUES "
            "(%s, %s)", params=(bundleid, self.reponame))
        for node in nodes:
            self.sqlcursor.execute(
                "INSERT INTO nodestobundle(node, bundle, reponame) "
                "VALUES (%s, %s, %s) ON DUPLICATE KEY UPDATE "
                "bundle=VALUES(bundle)",
                params=(node, bundleid, self.reponame))
        if commit:
            self.sqlconn.commit()

    def addbookmark(self, bookmark, node, commit=True):
        """Takes a bookmark name and hash, and records mapping in the metadata
        store."""
        if not self._connected:
            self.sqlconnect()
        self.log.info(
            "ADD BOOKMARKS %r bookmark: %r node: %r" %
            (self.reponame, bookmark, node))
        self.sqlcursor.execute(
            "INSERT INTO bookmarkstonode(bookmark, node, reponame) "
            "VALUES (%s, %s, %s) ON DUPLICATE KEY UPDATE node=VALUES(node)",
            params=(bookmark, node, self.reponame))
        if commit:
            self.sqlconn.commit()

    def addbookmarkandbundle(self, bundleid, nodes, bookmark, bookmarknode):
        if not self._connected:
            self.sqlconnect()
        self.addbundle(bundleid, nodes, commit=False)
        self.addbookmark(bookmark, bookmarknode, commit=False)
        self.sqlconn.commit()

    def listbookmarks(self):
        if not self._connected:
            self.sqlconnect()
        self.log.info("LIST BOOKMARKS")
        self.sqlcursor.execute("SELECT bookmark, node from bookmarkstonode")
        result = self.sqlcursor.fetchall()
        if not self._connected:
            self.sqlconnect()
        self.log.info("Found %d bookmarks")
        bookmarks = {}
        for row in result:
            bookmarks[row[0]] = row[1]
        return bookmarks

    def getbundle(self, node):
        """Returns the bundleid for the bundle that contains the given node."""
        if not self._connected:
            self.sqlconnect()
        self.log.info("GET BUNDLE %r %r" % (self.reponame, node))
        self.sqlcursor.execute(
            "SELECT bundle from nodestobundle "
            "WHERE node = %s AND reponame = %s", params=(node, self.reponame))
        result = self.sqlcursor.fetchall()
        if len(result) != 1 or len(result[0]) != 1:
            self.log.info("No matching node")
            return None
        bundle = result[0][0]
        self.log.info("Found bundle %r" % bundle)
        return bundle

    def getnode(self, bookmark):
        """Returns the node for the given bookmark. None if it doesn't exist."""
        if not self._connected:
            self.sqlconnect()
        self.log.info(
            "GET NODE reponame: %r bookmark: %r" % (self.reponame, bookmark))
        self.sqlcursor.execute(
            "SELECT node from bookmarkstonode WHERE "
            "bookmark = %s AND reponame = %s", params=(bookmark, self.reponame))
        result = self.sqlcursor.fetchall()
        if len(result) != 1 or len(result[0]) != 1:
            self.log.info("No matching bookmark")
            return None
        bookmark = result[0][0]
        self.log.info("Found node %r" % bookmark)
        return bookmark

class fileindexapi(indexapi):
    def __init__(self, repo):
        super(fileindexapi, self).__init__()
        self._repo = repo
        root = repo.ui.config('infinitepush', 'indexpath')
        if not root:
            root = os.path.join('scratchbranches', 'index')

        self._nodemap = os.path.join(root, 'nodemap')
        self._bookmarkmap = os.path.join(root, 'bookmarkmap')

    def addbundle(self, bundleid, nodes):
        for node in nodes:
            nodepath = os.path.join(self._nodemap, node)
            self._write(nodepath, bundleid)

    def addbookmark(self, bookmark, node):
        bookmarkpath = os.path.join(self._bookmarkmap, bookmark)
        self._write(bookmarkpath, node)

    def addbookmarkandbundle(self, bundleid, nodes, bookmark, bookmarknode):
        self.addbookmark(bookmark, bookmarknode)
        self.addbundle(bundleid, nodes)

    def getbundle(self, node):
        nodepath = os.path.join(self._nodemap, node)
        return self._read(nodepath)

    def getnode(self, bookmark):
        bookmarkpath = os.path.join(self._bookmarkmap, bookmark)
        return self._read(bookmarkpath)

    def _write(self, path, value):
        vfs = self._repo.vfs
        dirname = vfs.dirname(path)
        if not vfs.exists(dirname):
            vfs.makedirs(dirname)

        vfs.write(path, value)

    def _read(self, path):
        vfs = self._repo.vfs
        if not vfs.exists(path):
            return None
        return vfs.read(path)

class CustomConverter(mysql.connector.conversion.MySQLConverter):
    """Ensure that all values being returned are returned as python string
    (versus the default byte arrays)."""
    def _STRING_to_python(self, value, dsc=None):
        return str(value)

    def _VAR_STRING_to_python(self, value, dsc=None):
        return str(value)

    def _BLOB_to_python(self, value, dsc=None):
        return str(value)
