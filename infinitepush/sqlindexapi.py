# Infinite push
#
# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import os
import time
import logging

import warnings
import mysql.connector

from indexapi import (
    indexapi,
    indexexception,
)

def _convertbookmarkpattern(pattern):
    pattern = pattern.replace('_', '\\_')
    pattern = pattern.replace('%', '\\%')
    if pattern.endswith('*'):
        pattern = pattern[:-1] + '%'
    return pattern

class sqlindexapi(indexapi):
    '''
    Sql backend for infinitepush index. See schema.sql
    '''

    def __init__(self, reponame, host, port,
                 database, user, password, logfile, loglevel,
                 waittimeout=300, locktimeout=120):
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
        self._waittimeout = waittimeout
        self._locktimeout = locktimeout

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

        waittimeout = self.sqlconn.converter.escape('%s' % self._waittimeout)

        self.sqlcursor = self.sqlconn.cursor()
        self.sqlcursor.execute("SET wait_timeout=%s" % waittimeout)
        self.sqlcursor.execute("SET innodb_lock_wait_timeout=%s" %
                               self._locktimeout)
        self._connected = True

    def close(self):
        """Cleans up the metadata store connection."""
        with warnings.catch_warnings():
            warnings.simplefilter("ignore")
            self.sqlcursor.close()
            self.sqlconn.close()
        self.sqlcursor = None
        self.sqlconn = None

    def __enter__(self):
        if not self._connected:
            self.sqlconnect()
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        if exc_type is None:
            self.sqlconn.commit()
        else:
            self.sqlconn.rollback()

    def addbundle(self, bundleid, nodesctx):
        if not self._connected:
            self.sqlconnect()
        self.log.info("ADD BUNDLE %r %r" % (self.reponame, bundleid))
        self.sqlcursor.execute(
            "INSERT INTO bundles(bundle, reponame) VALUES "
            "(%s, %s)", params=(bundleid, self.reponame))
        for ctx in nodesctx:
            self.sqlcursor.execute(
                "INSERT INTO nodestobundle(node, bundle, reponame) "
                "VALUES (%s, %s, %s) ON DUPLICATE KEY UPDATE "
                "bundle=VALUES(bundle)",
                params=(ctx.hex(), bundleid, self.reponame))

            extra = ctx.extra()
            author_name = ctx.user()
            committer_name = extra.get('committer', ctx.user())
            author_date = int(ctx.date()[0])
            committer_date = int(extra.get('committer_date', author_date))
            self.sqlcursor.execute(
                "INSERT IGNORE INTO nodesmetadata(node, message, p1, p2, "
                "author, committer, author_date, committer_date, "
                "reponame) VALUES "
                "(%s, %s, %s, %s, %s, %s, %s, %s, %s)",
                params=(ctx.hex(), ctx.description(),
                        ctx.p1().hex(), ctx.p2().hex(), author_name,
                        committer_name, author_date, committer_date,
                        self.reponame)
            )

    def addbookmark(self, bookmark, node):
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

    def addmanybookmarks(self, bookmarks):
        if not self._connected:
            self.sqlconnect()
        args = []
        values = []
        for bookmark, node in bookmarks.iteritems():
            args.append('(%s, %s, %s)')
            values.extend((bookmark, node, self.reponame))
        args = ','.join(args)

        self.sqlcursor.execute(
            "INSERT INTO bookmarkstonode(bookmark, node, reponame) "
            "VALUES %s ON DUPLICATE KEY UPDATE node=VALUES(node)" % args,
            params=values)

    def deletebookmarks(self, patterns):
        """Accepts list of bookmark patterns and deletes them.
        If `commit` is set then bookmark will actually be deleted. Otherwise
        deletion will be delayed until the end of transaction.
        """
        if not self._connected:
            self.sqlconnect()
        self.log.info("DELETE BOOKMARKS: %s" % patterns)
        for pattern in patterns:
            pattern = _convertbookmarkpattern(pattern)
            self.sqlcursor.execute(
                "DELETE from bookmarkstonode WHERE bookmark LIKE (%s) "
                "and reponame = %s",
                params=(pattern, self.reponame))

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
        node = result[0][0]
        self.log.info("Found node %r" % node)
        return node

    def getbookmarks(self, query):
        if not self._connected:
            self.sqlconnect()
        self.log.info(
            "QUERY BOOKMARKS reponame: %r query: %r" % (self.reponame, query))
        query = _convertbookmarkpattern(query)
        self.sqlcursor.execute(
            "SELECT bookmark, node from bookmarkstonode WHERE "
            "reponame = %s AND bookmark LIKE %s",
            params=(self.reponame, query))
        result = self.sqlcursor.fetchall()
        bookmarks = {}
        for row in result:
            if len(row) != 2:
                self.log.info("Bad row returned: %s" % row)
                continue
            bookmarks[row[0]] = row[1]
        return bookmarks

    def saveoptionaljsonmetadata(self, node, jsonmetadata):
        if not self._connected:
            self.sqlconnect()
        self.log.info(
            ("INSERT METADATA, QUERY BOOKMARKS reponame: %r " +
             "node: %r, jsonmetadata: %s") %
            (self.reponame, node, jsonmetadata))

        self.sqlcursor.execute(
            "UPDATE nodesmetadata SET optional_json_metadata=%s WHERE node=%s",
            params=(jsonmetadata, node))

class CustomConverter(mysql.connector.conversion.MySQLConverter):
    """Ensure that all values being returned are returned as python string
    (versus the default byte arrays)."""
    def _STRING_to_python(self, value, dsc=None):
        return str(value)

    def _VAR_STRING_to_python(self, value, dsc=None):
        return str(value)

    def _BLOB_to_python(self, value, dsc=None):
        return str(value)
