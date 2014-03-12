#!/usr/bin/env python
"""
Tests the behaviour of filelog w.r.t. data starting with '\1\n'
"""
from mercurial import ui, hg
from mercurial.node import nullid, hex

myui = ui.ui()
repo = hg.repository(myui, path='.', create=True)

fl = repo.file('foobar')

def addrev(text, renamed=False):
    if renamed:
        # data doesn't matter. Just make sure filelog.renamed() returns True
        meta = {'copyrev': hex(nullid), 'copy': 'bar'}
    else:
        meta = {}

    lock = t = None
    try:
        lock = repo.lock()
        t = repo.transaction('commit')
        node = fl.add(text, meta, t, 0, nullid, nullid)
        return node
    finally:
        if t:
            t.close()
        if lock:
            lock.release()

def error(text):
    print 'ERROR: ' + text

textwith = '\1\nfoo'
without = 'foo'

node = addrev(textwith)
if not textwith == fl.read(node):
    error('filelog.read for data starting with \\1\\n')
if fl.cmp(node, textwith) or not fl.cmp(node, without):
    error('filelog.cmp for data starting with \\1\\n')
if fl.size(0) != len(textwith):
    error('FIXME: This is a known failure of filelog.size for data starting '
        'with \\1\\n')

node = addrev(textwith, renamed=True)
if not textwith == fl.read(node):
    error('filelog.read for a renaming + data starting with \\1\\n')
if fl.cmp(node, textwith) or not fl.cmp(node, without):
    error('filelog.cmp for a renaming + data starting with \\1\\n')
if fl.size(1) != len(textwith):
    error('filelog.size for a renaming + data starting with \\1\\n')

print 'OK.'
