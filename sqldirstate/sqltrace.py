# sqltrace.py - sqlite command tracing for logging purposes
#
# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
import sys

def tracewrapsqlconn(sqlconn, tracefile):
    if tracefile == '-':
        f = sys.stderr
    else:
        f = open(tracefile, 'a+')

    def logquery(q, paramtuple=()):
        # This is module for escaping the sql queries imported from postressql
        # as sqllite doesn't provide one.  Still better than pretending to
        # escape it manually over here.
        #
        # We are importing it here as it's only used for debugging purposes so
        # it's not considered as dependency for prod system.
        from psycopg2.extensions import adapt
        out = q.replace("?", "%s") % tuple(map(adapt, paramtuple))
        f.write(out)
        f.write(';\n')

    class CursorWrapper(object):
        def __init__(self, cursor):
            self._cursor = cursor

        def __getattr__(self, attr):
            return self._cursor.__getattribute__(attr)

        def __iter__(self, *args, **kwargs):
            return self._cursor.__iter__(*args, **kwargs)

        def iter(self, *args, **kwargs):
            return self._cursor.iter(*args, **kwargs)

        def execute(self, sql, *params):
            logquery(sql, *params)
            return self._cursor.execute(sql, *params)

        def executemany(self, sql, paramslist):
            for params in paramslist:
                logquery(sql, params)
            return self._cursor.executemany(sql, paramslist)

        def executescript(self, sql, *params):
            logquery(sql)
            return self._cursor.executescript(sql, *params)

    class ConnWrapper(object):
        def __init__(self, wrapped):
            self._sqlconn = wrapped

        def __getattr__(self, attr):
            return self._sqlconn.__getattribute__(attr)

        def cursor(self, *args, **kwargs):
            return CursorWrapper(self._sqlconn.cursor())

        def execute(self, sql, *params):
            logquery(sql, *params)
            return self._sqlconn.execute(sql, *params)

        def executemany(self, sql, paramslist):
            for params in paramslist:
                logquery(sql, params)
            return self._sqlconn.executemany(sql, paramslist)

        def executescript(self, sql, *params):
            logquery(sql)
            return self._sqlconn.executescript(sql, *params)
    return ConnWrapper(sqlconn)
