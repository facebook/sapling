from __future__ import absolute_import, print_function

import os
import sys
import tempfile

# make it runnable using python directly without run-tests.py
sys.path[0:0] = [os.path.join(os.path.dirname(__file__), '..', 'hgext3rd')]

import linkrevcache

def genhsh(i):
    return chr(i) + b'\0' * 19

def ensure(condition):
    if not condition:
        raise RuntimeError('Unexpected')

def testbasicreadwrite():
    path = tempfile.mkdtemp()
    db = linkrevcache.linkrevdb(path, write=True)

    ensure(db.getlastrev() == 0)
    for i in xrange(25):
        fname = str(i % 5)
        fnode = genhsh(i // 5)
        ensure(db.getlinkrevs(fname, fnode) == [])
        db.appendlinkrev(fname, fnode, i)
        ensure(db.getlinkrevs(fname, fnode) == [i])
        db.appendlinkrev(fname, fnode, i)
        db.appendlinkrev(fname, fnode, i + 1)
        db.appendlinkrev(fname, fnode, i)
        ensure(db.getlinkrevs(fname, fnode) == [i, i + 1])

        db.setlastrev(i)
        ensure(db.getlastrev() == i)

    db.close()

    # re-open for reading
    db = linkrevcache.linkrevdb(path)
    ensure(db.getlastrev() == 24)
    for i in xrange(25):
        fname = str(i % 5)
        fnode = genhsh(i // 5)
        ensure(db.getlinkrevs(fname, fnode) == [i, i + 1])

    for i in xrange(26, 50):
        fname = str(i % 5)
        fnode = genhsh(i // 5)
        ensure(db.getlinkrevs(fname, fnode) == [])

testbasicreadwrite()
