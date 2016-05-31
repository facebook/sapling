# sqlmap.py - sql backed dictionary
#
# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from abc import abstractmethod, ABCMeta
import collections

from mercurial import parsers

dirstatetuple = parsers.dirstatetuple

class sqlmap(collections.MutableMapping):
    """ a dictionary-like object backed by sqllite db."""
    __metaclass__ = ABCMeta

    def __init__(self, sqlconn):
        self._sqlconn = sqlconn
        self.createschema()

    @abstractmethod
    def createschema(self):
        """ create db table if doesn't exist """
        pass

    @abstractmethod
    def dropschema(self):
        """ drop db table """
        pass

    def _rowtovalue(self, row):
        """ converts row of db to a value format """
        return row[0]

    def _valuetorow(self, value):
        """ convers provided value to db row format """
        return (value,)

    @property
    def _numcols(self):
        return 1 + len(self._valuenames)

    @property
    def _valuenamesstr(self):
        return ', '.join(self._valuenames)

    @property
    def _querytemplateargs(self):
        return {'table': self._tablename,
                'keyname': self._keyname,
                'valuenames': self._valuenamesstr,
                'placeholders': ', '.join(['?'] * self._numcols)}

    def __setitem__(self, key, item):
        cur = self._sqlconn.cursor()

        item = self._valuetorow(item)


        cur.execute('''INSERT OR REPLACE INTO {table} ({keyname}, {valuenames})
            VALUES ({placeholders})'''.format(**self._querytemplateargs),
            (key,) + item)
        cur.close()

    def __getitem__(self, key):
        cur = self._sqlconn.cursor()
        cur.execute('''SELECT {valuenames} FROM {table}
                    WHERE {keyname}=?'''.format(**self._querytemplateargs),
                    (key,))
        row = cur.fetchone()
        cur.close()

        if row is None:
            raise KeyError("key %s not found" % key)
        return self._rowtovalue(row)

    def __delitem__(self, key):
        cur = self._sqlconn.cursor()
        cur.execute('''DELETE FROM {table} WHERE {keyname}=?'''.format(
            **self._querytemplateargs), (key,))

        if cur.rowcount == 0:
            raise KeyError("key %s not found" % key)
        cur.close()

    def __len__(self):
        cur = self._sqlconn.cursor()
        cur.execute('''SELECT COUNT(*) FROM {table}'''.format(
            **self._querytemplateargs))
        res = cur.fetchone()
        cur.close()
        return res[0]

    def clear(self):
        cur = self._sqlconn.cursor()
        cur.execute('''DELETE FROM {table}'''.format(**self._querytemplateargs))
        cur.close()

    def copy(self):
        return dict(self)

    def _update(self, otherdict):
        tuplelist = [(k,) + self._valuetorow(v)
                     for k, v in otherdict.iteritems()]

        cur = self._sqlconn.cursor()
        cur.executemany('''INSERT OR REPLACE INTO {table}
            ({keyname}, {valuenames}) VALUES ({placeholders})'''.format(
            **self._querytemplateargs), tuplelist)
        cur.close()

    def update(self, *args, **kwargs):
        assert len(args) == 1 or kwargs
        if args:
            self._update(args[0])
        if kwargs:
            self._update(kwargs)

    def keys(self):
        cur = self._sqlconn.cursor()
        cur.execute('''SELECT {keyname} FROM {table}'''.format(
            **self._querytemplateargs))
        keys = cur.fetchall()
        cur.close()
        return [k[0] for k in keys]

    def __iter__(self):
        cur = self._sqlconn.cursor()
        cur.execute('''SELECT {keyname} FROM {table}'''.format(
            **self._querytemplateargs))
        for r in cur:
            yield r[0]
        cur.close()

    def iteritems(self):
        cur = self._sqlconn.cursor()
        cur.execute('''SELECT {keyname}, {valuenames} from {table}'''.format(
            **self._querytemplateargs))
        for r in cur:
            yield (r[0], self._rowtovalue(r[1:]))
        cur.close()
