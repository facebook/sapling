# Infinite push
#
# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import logging
import os
import time
import warnings

import mysql.connector
from indexapi import indexapi, indexexception
from mercurial import util


def _convertbookmarkpattern(pattern):
    # To search for \, specify it as \\
    # To search for _, specify it as \_
    # To search for %, specify it as \%
    pattern = pattern.replace("\\", "\\\\")
    pattern = pattern.replace("_", "\\_")
    pattern = pattern.replace("%", "\\%")
    if pattern.endswith("*"):
        pattern = pattern[:-1] + "%"
    return pattern


SEC_IN_DAY = 24 * 60 * 60


class sqlindexapi(indexapi):
    """
    Sql backend for infinitepush index. See schema.sql
    """

    def __init__(
        self,
        reponame,
        host,
        port,
        database,
        user,
        password,
        logfile,
        loglevel,
        shorthasholdrevthreshold,
        waittimeout=300,
        locktimeout=120,
    ):
        super(sqlindexapi, self).__init__()
        self.reponame = reponame
        self.sqlargs = {
            "host": host,
            "port": port,
            "database": database,
            "user": user,
            "password": password,
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
        self.shorthasholdrevthreshold = shorthasholdrevthreshold

    def sqlconnect(self):
        if self.sqlconn:
            raise indexexception("SQL connection already open")
        if self.sqlcursor:
            raise indexexception("SQL cursor already open without connection")
        retry = 3
        while True:
            try:
                self.sqlconn = mysql.connector.connect(force_ipv6=True, **self.sqlargs)

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

        waittimeout = self.sqlconn.converter.escape("%s" % self._waittimeout)

        self.sqlcursor = self.sqlconn.cursor()
        self.sqlcursor.execute("SET wait_timeout=%s" % waittimeout)
        self.sqlcursor.execute("SET innodb_lock_wait_timeout=%s" % self._locktimeout)
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
        """Records bundles, mapping from node to bundle and metadata for nodes
        """
        if not self._connected:
            self.sqlconnect()

        # insert bundle
        self.log.info("ADD BUNDLE %r %r" % (self.reponame, bundleid))
        self.sqlcursor.execute(
            "INSERT INTO bundles(bundle, reponame) VALUES " "(%s, %s)",
            params=(bundleid, self.reponame),
        )

        # insert nodes to bundle mapping

        self.sqlcursor.executemany(
            "INSERT INTO nodestobundle(node, bundle, reponame) "
            "VALUES (%s, %s, %s) ON DUPLICATE KEY UPDATE "
            "bundle=VALUES(bundle)",
            [(ctx.hex(), bundleid, self.reponame) for ctx in nodesctx],
        )

        # insert metadata
        data = [
            (
                ctx.hex(),  # node
                ctx.description(),  # message
                ctx.p1().hex(),  # p1
                ctx.p2().hex(),  # p2
                ctx.user(),  # author
                ctx.extra().get("committer", ctx.user()),  # committer
                int(ctx.date()[0]),  # author_date
                int(
                    ctx.extra().get("committer_date", int(ctx.date()[0]))
                ),  # committer_date
                self.reponame,  # reponame
            )
            for ctx in nodesctx
        ]

        self.sqlcursor.executemany(
            "INSERT IGNORE INTO nodesmetadata(node, message, p1, p2, "
            "author, committer, author_date, committer_date, "
            "reponame) VALUES "
            "(%s, %s, %s, %s, %s, %s, %s, %s, %s)",
            data,
        )

    def addbookmark(self, bookmark, node):
        """Takes a bookmark name and hash, and records mapping in the metadata
        store."""
        if not self._connected:
            self.sqlconnect()
        self.log.info(
            "ADD BOOKMARKS %r bookmark: %r node: %r" % (self.reponame, bookmark, node)
        )
        self.sqlcursor.execute(
            "INSERT INTO bookmarkstonode(bookmark, node, reponame) "
            "VALUES (%s, %s, %s) ON DUPLICATE KEY UPDATE node=VALUES(node)",
            params=(bookmark, node, self.reponame),
        )

    def addmanybookmarks(self, bookmarks):
        """Records mapping of bookmarks and nodes"""
        if not self._connected:
            self.sqlconnect()

        data = [
            (bookmark, node, self.reponame) for bookmark, node in bookmarks.iteritems()
        ]

        self.sqlcursor.executemany(
            "INSERT INTO bookmarkstonode(bookmark, node, reponame) "
            "VALUES (%s, %s, %s) ON DUPLICATE KEY UPDATE node=VALUES(node)",
            data,
        )

    def deletebookmarks(self, patterns):
        """Accepts list of bookmark patterns and deletes them.
        If `commit` is set then bookmark will actually be deleted. Otherwise
        deletion will be delayed until the end of transaction.
        """
        if not self._connected:
            self.sqlconnect()

        # build and execute detete query
        self.log.info("DELETE BOOKMARKS: %s" % patterns)
        patterns = [_convertbookmarkpattern(pattern) for pattern in patterns]
        condition1 = "reponame = %s"
        condition2 = " OR ".join(("bookmark LIKE (%s)",) * len(patterns))
        query = "DELETE FROM bookmarkstonode WHERE (%s) AND (%s)" % (
            condition1,
            condition2,
        )
        self.sqlcursor.execute(query, params=[self.reponame] + patterns)

    def getbundle(self, node):
        """Returns the bundleid for the bundle that contains the given node."""
        if not self._connected:
            self.sqlconnect()
        self.log.info("GET BUNDLE %r %r" % (self.reponame, node))
        self.sqlcursor.execute(
            "SELECT bundle from nodestobundle " "WHERE node = %s AND reponame = %s",
            params=(node, self.reponame),
        )
        result = self.sqlcursor.fetchall()
        if len(result) != 1 or len(result[0]) != 1:
            self.log.info("No matching node")
            return None
        bundle = result[0][0]
        self.log.info("Found bundle %r" % bundle)
        return bundle

    def getnodebyprefix(self, prefix):
        """Returns the node with the given hash prefix.
        None if it doesn't exist.
        Raise error for ambiguous identifier"""
        if not self._connected:
            self.sqlconnect()
        self.log.info("GET NODE BY PREFIX %r %r" % (self.reponame, prefix))
        nodeprefixpattern = prefix + "%"
        result = None

        if len(prefix) >= 6 and len(prefix) < 20:
            # With longer hashes we can make more complex QUERY
            # in order to return some suggestions with the matched PREFIX
            # so user can pick up the desired one easily
            # there is no need to go this path for prefixes longer than 20
            # because to find several commits is highly unlikely
            # Order suggestions by date to show the recent ones first
            cmd = (
                "SELECT t1.node, t2.message, t2.author, t2.committer_date "
                "FROM nodestobundle t1 JOIN nodesmetadata t2 "
                "ON t1.node = t2.node AND t1.reponame = t2.reponame "
                "WHERE t1.node LIKE %s AND t1.reponame =  %s "
                "ORDER BY t2.committer_date DESC LIMIT 5"
            )
            params = (nodeprefixpattern, self.reponame)
            self.sqlcursor.execute(cmd, params)
            result = self.sqlcursor.fetchall()

            def gettitle(s):
                return s.splitlines()[0]

            # format time from timestamp
            def formattime(s):
                _timeformat = r"%d %b %Y %H:%M"
                return time.strftime(_timeformat, time.localtime(int(s)))

            # format metadata output from query rows
            def formatdata(arr):
                return "\n".join(
                    [
                        "  changeset: {c}\n"
                        "  author: {a}\n"
                        "  date: {d}\n"
                        "  summary: {m}\n".format(
                            c=c, m=gettitle(m), a=a, d=formattime(d)
                        )
                        for c, m, a, d in result
                    ]
                )

            if len(result) > 1:
                raise indexexception(
                    ("ambiguous identifier '%s'\n" % prefix)
                    + "#commitcloud suggestions are:\n"
                    + formatdata(result)
                )

            if len(result) == 1:
                revdate = result[0][3]
                threshold = self.shorthasholdrevthreshold * SEC_IN_DAY
                if time.time() - revdate > threshold:
                    raise indexexception(
                        "commit '%s' is more than %d days old\n"
                        "description:\n%s"
                        "#commitcloud hint: if you would like to fetch this "
                        "commit, please provide the full hash"
                        % (prefix, self.shorthasholdrevthreshold, formatdata(result))
                    )

        else:
            self.sqlcursor.execute(
                "SELECT node from nodestobundle "
                "WHERE node LIKE %s "
                "AND reponame = %s "
                "LIMIT 2",
                params=(nodeprefixpattern, self.reponame),
            )
            result = self.sqlcursor.fetchall()

            if len(result) > 1:
                raise indexexception(
                    ("ambiguous identifier '%s'\n" % prefix)
                    + "suggestion: provide longer commithash prefix"
                )

        # result not found
        if len(result) != 1 or len(result[0]) == 0:
            self.log.info("No matching node")
            return None

        node = result[0][0]

        # Log found result. It is unique.
        self.log.info("Found node %r" % node)
        return node

    def getnode(self, bookmark):
        """Returns the node for the given bookmark. None if it doesn't exist."""
        if not self._connected:
            self.sqlconnect()
        self.log.info("GET NODE reponame: %r bookmark: %r" % (self.reponame, bookmark))
        self.sqlcursor.execute(
            "SELECT node from bookmarkstonode WHERE " "bookmark = %s AND reponame = %s",
            params=(bookmark, self.reponame),
        )
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
        self.log.info("QUERY BOOKMARKS reponame: %r query: %r" % (self.reponame, query))
        query = _convertbookmarkpattern(query)
        self.sqlcursor.execute(
            "SELECT bookmark, node from bookmarkstonode WHERE "
            "reponame = %s AND bookmark LIKE %s "
            # Bookmarks have to be restored in the same order of creation
            # See T24417531
            "ORDER BY time ASC",
            params=(self.reponame, query),
        )
        result = self.sqlcursor.fetchall()
        bookmarks = util.sortdict()
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
            (
                "INSERT METADATA, QUERY BOOKMARKS reponame: %r "
                + "node: %r, jsonmetadata: %s"
            )
            % (self.reponame, node, jsonmetadata)
        )

        self.sqlcursor.execute(
            "UPDATE nodesmetadata SET optional_json_metadata=%s WHERE "
            "reponame=%s AND node=%s",
            params=(jsonmetadata, self.reponame, node),
        )


class CustomConverter(mysql.connector.conversion.MySQLConverter):
    """Ensure that all values being returned are returned as python string
    (versus the default byte arrays)."""

    def _STRING_to_python(self, value, dsc=None):
        return str(value)

    def _VAR_STRING_to_python(self, value, dsc=None):
        return str(value)

    def _BLOB_to_python(self, value, dsc=None):
        return str(value)
