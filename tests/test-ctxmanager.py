from __future__ import absolute_import

import silenttestrunner
import unittest

from mercurial import util

class contextmanager(object):
    def __init__(self, name, trace):
        self.name = name
        self.entered = False
        self.exited = False
        self.trace = trace

    def __enter__(self):
        self.entered = True
        self.trace(('enter', self.name))
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        self.exited = exc_type, exc_val, exc_tb
        self.trace(('exit', self.name))

    def __repr__(self):
        return '<ctx %r>' % self.name

class ctxerror(Exception):
    pass

class raise_on_enter(contextmanager):
    def __enter__(self):
        self.trace(('raise', self.name))
        raise ctxerror(self.name)

class raise_on_exit(contextmanager):
    def __exit__(self, exc_type, exc_val, exc_tb):
        self.trace(('raise', self.name))
        raise ctxerror(self.name)

def ctxmgr(name, trace):
    return lambda: contextmanager(name, trace)

class test_ctxmanager(unittest.TestCase):
    def test_basics(self):
        trace = []
        addtrace = trace.append
        with util.ctxmanager(ctxmgr('a', addtrace), ctxmgr('b', addtrace)) as c:
            a, b = c.enter()
            c.atexit(addtrace, ('atexit', 'x'))
            c.atexit(addtrace, ('atexit', 'y'))
        self.assertEqual(trace, [('enter', 'a'), ('enter', 'b'),
                                 ('atexit', 'y'), ('atexit', 'x'),
                                 ('exit', 'b'), ('exit', 'a')])

    def test_raise_on_enter(self):
        trace = []
        addtrace = trace.append
        def go():
            with util.ctxmanager(ctxmgr('a', addtrace),
                                 lambda: raise_on_enter('b', addtrace)) as c:
                c.enter()
                addtrace('unreachable')
        self.assertRaises(ctxerror, go)
        self.assertEqual(trace, [('enter', 'a'), ('raise', 'b'), ('exit', 'a')])

    def test_raise_on_exit(self):
        trace = []
        addtrace = trace.append
        def go():
            with util.ctxmanager(ctxmgr('a', addtrace),
                                 lambda: raise_on_exit('b', addtrace)) as c:
                c.enter()
                addtrace('running')
        self.assertRaises(ctxerror, go)
        self.assertEqual(trace, [('enter', 'a'), ('enter', 'b'), 'running',
                                 ('raise', 'b'), ('exit', 'a')])

if __name__ == '__main__':
    silenttestrunner.main(__name__)
