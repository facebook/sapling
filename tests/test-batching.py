# test-batching.py - tests for transparent command batching
#
# Copyright 2011 Peter Arrenbrecht <peter@arrenbrecht.ch>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import, print_function

from mercurial import (
    peer,
    wireproto,
)

# equivalent of repo.repository
class thing(object):
    def hello(self):
        return "Ready."

# equivalent of localrepo.localrepository
class localthing(thing):
    def foo(self, one, two=None):
        if one:
            return "%s and %s" % (one, two,)
        return "Nope"
    def bar(self, b, a):
        return "%s und %s" % (b, a,)
    def greet(self, name=None):
        return "Hello, %s" % name
    def batch(self):
        '''Support for local batching.'''
        return peer.localbatch(self)

# usage of "thing" interface
def use(it):

    # Direct call to base method shared between client and server.
    print(it.hello())

    # Direct calls to proxied methods. They cause individual roundtrips.
    print(it.foo("Un", two="Deux"))
    print(it.bar("Eins", "Zwei"))

    # Batched call to a couple of (possibly proxied) methods.
    batch = it.batch()
    # The calls return futures to eventually hold results.
    foo = batch.foo(one="One", two="Two")
    foo2 = batch.foo(None)
    bar = batch.bar("Eins", "Zwei")
    # We can call non-batchable proxy methods, but the break the current batch
    # request and cause additional roundtrips.
    greet = batch.greet(name="John Smith")
    # We can also add local methods into the mix, but they break the batch too.
    hello = batch.hello()
    bar2 = batch.bar(b="Uno", a="Due")
    # Only now are all the calls executed in sequence, with as few roundtrips
    # as possible.
    batch.submit()
    # After the call to submit, the futures actually contain values.
    print(foo.value)
    print(foo2.value)
    print(bar.value)
    print(greet.value)
    print(hello.value)
    print(bar2.value)

# local usage
mylocal = localthing()
print()
print("== Local")
use(mylocal)

# demo remoting; mimicks what wireproto and HTTP/SSH do

# shared

def escapearg(plain):
    return (plain
            .replace(':', '::')
            .replace(',', ':,')
            .replace(';', ':;')
            .replace('=', ':='))
def unescapearg(escaped):
    return (escaped
            .replace(':=', '=')
            .replace(':;', ';')
            .replace(':,', ',')
            .replace('::', ':'))

# server side

# equivalent of wireproto's global functions
class server(object):
    def __init__(self, local):
        self.local = local
    def _call(self, name, args):
        args = dict(arg.split('=', 1) for arg in args)
        return getattr(self, name)(**args)
    def perform(self, req):
        print("REQ:", req)
        name, args = req.split('?', 1)
        args = args.split('&')
        vals = dict(arg.split('=', 1) for arg in args)
        res = getattr(self, name)(**vals)
        print("  ->", res)
        return res
    def batch(self, cmds):
        res = []
        for pair in cmds.split(';'):
            name, args = pair.split(':', 1)
            vals = {}
            for a in args.split(','):
                if a:
                    n, v = a.split('=')
                    vals[n] = unescapearg(v)
            res.append(escapearg(getattr(self, name)(**vals)))
        return ';'.join(res)
    def foo(self, one, two):
        return mangle(self.local.foo(unmangle(one), unmangle(two)))
    def bar(self, b, a):
        return mangle(self.local.bar(unmangle(b), unmangle(a)))
    def greet(self, name):
        return mangle(self.local.greet(unmangle(name)))
myserver = server(mylocal)

# local side

# equivalent of wireproto.encode/decodelist, that is, type-specific marshalling
# here we just transform the strings a bit to check we're properly en-/decoding
def mangle(s):
    return ''.join(chr(ord(c) + 1) for c in s)
def unmangle(s):
    return ''.join(chr(ord(c) - 1) for c in s)

# equivalent of wireproto.wirerepository and something like http's wire format
class remotething(thing):
    def __init__(self, server):
        self.server = server
    def _submitone(self, name, args):
        req = name + '?' + '&'.join(['%s=%s' % (n, v) for n, v in args])
        return self.server.perform(req)
    def _submitbatch(self, cmds):
        req = []
        for name, args in cmds:
            args = ','.join(n + '=' + escapearg(v) for n, v in args)
            req.append(name + ':' + args)
        req = ';'.join(req)
        res = self._submitone('batch', [('cmds', req,)])
        return res.split(';')

    def batch(self):
        return wireproto.remotebatch(self)

    @peer.batchable
    def foo(self, one, two=None):
        if not one:
            yield "Nope", None
        encargs = [('one', mangle(one),), ('two', mangle(two),)]
        encresref = peer.future()
        yield encargs, encresref
        yield unmangle(encresref.value)

    @peer.batchable
    def bar(self, b, a):
        encresref = peer.future()
        yield [('b', mangle(b),), ('a', mangle(a),)], encresref
        yield unmangle(encresref.value)

    # greet is coded directly. It therefore does not support batching. If it
    # does appear in a batch, the batch is split around greet, and the call to
    # greet is done in its own roundtrip.
    def greet(self, name=None):
        return unmangle(self._submitone('greet', [('name', mangle(name),)]))

# demo remote usage

myproxy = remotething(myserver)
print()
print("== Remote")
use(myproxy)
