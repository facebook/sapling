# simplecache.py - cache slow things locally so they are fast the next time
#
# Copyright 2014 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""
simplecache is a dirt-simple cache of various functions that get slow in large
repositories. It is aimed at speeding up common operations that programmers
often take, like diffing two revisions (eg, hg export).

Currently we cache the full results of these functions:
    copies.pathcopies (a dictionary)
    context.basectx._buildstatus (a scmutil.status object -- a tuple of lists)
"""

import socket, json
from mercurial import extensions, node, copies, context
from mercurial.scmutil import status

testedwith = 'internal'

def extsetup(ui):
    extensions.wrapfunction(copies, 'pathcopies', pathcopiesui(ui))
    extensions.wrapfunction(context.basectx, '_buildstatus', buildstatusui(ui))

def getmcsock(ui):
    """
    Return a socket opened up to talk to localhost mcrouter.
    """
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    host = ui.config('simplecache', 'host', default='localhost')
    port = int(ui.config('simplecache', 'port', default=11101))
    s.connect((host, port))
    return s

def mcget(key, ui):
    """
    Use local mcrouter to get a key from memcache
    """
    if type(key) != str:
        raise ValueError('Key must be a string')
    s = getmcsock(ui)
    s.sendall('get %s\r\n' % key)
    meta = []
    value = None
    while True:
        char = s.recv(1)
        if char != '\r':
            meta.append(char)
        else:
            meta = ''.join(meta)
            if meta == 'END':
                break
            char = s.recv(1) # throw away newline
            _, key, flags, sz  = ''.join(meta).strip().split(' ')
            value = s.recv(int(sz))
            s.recv(7) # throw away \r\nEND\r\n
            break
    s.close()
    return value

def mcset(key, value, ui):
    """
    Use local mcrouter to set a key to memcache
    """
    if type(key) != str:
        raise ValueError('Key must be a string')
    if type(value) != str:
        raise ValueError('Value must be a string')

    sz = len(value)
    tmpl = 'set %s 0 0 %d\r\n%s\r\n'
    s = getmcsock(ui)
    s.sendall(tmpl % (key, sz, value))
    data = []
    while True:
        char = s.recv(1)
        if char not in '\r\n':
            data.append(char)
        else:
            break
    s.close()
    return ''.join(data) == 'STORED'

class pathcopiesserializer(object):
    """
    Serialize and deserialize the results of calls to copies.pathcopies.
    Results are just dictionaries, so this just uses json.
    """
    @staticmethod
    def serialize(copydict):
        encoded = dict((k.encode('base64'), v.encode('base64'))
                for (k, v) in copydict.iteritems())
        return json.dumps(encoded)

    @staticmethod
    def deserialize(string):
        encoded = json.loads(string)
        return dict((k.decode('base64'), v.decode('base64'))
                for k, v in encoded.iteritems())

def pathcopiesui(ui):
    version = ui.config('simplecache', 'version', default='1')
    def pathcopies(orig, x, y, match=None):
        func = lambda: orig(x, y, match=match)
        if x._node is not None and y._node is not None and not match:
            key = 'cca.hg.pathcopies:%s:%s:v%s' % (
                    node.hex(x._node), node.hex(y._node), version)
            return _mcmemoize(func, key, pathcopiesserializer, ui)
        return func()
    return pathcopies

class buildstatusserializer(object):
    """
    Serialize and deserialize the results of calls to buildstatus.
    Results are status objects, which extend tuple. Each status object
    has seven lists within it, each containing strings of filenames in
    each type of status.
    """
    @staticmethod
    def serialize(status):
        ls = [list(status[i]) for i in range(7)]
        ll = []
        for s in ls:
            ll.append([f.encode('base64') for f in s])
        return json.dumps(ll)

    @staticmethod
    def deserialize(string):
        ll = json.loads(string)
        ls = []
        for l in ll:
            ls.append([f.decode('base64') for f in l])
        return status(*ls)

def buildstatusui(ui):
    version = ui.config('simplecache', 'version', default='1')
    def buildstatus(orig, self, other, status, match, ignored, clean, unknown):
        func = lambda: orig(self, other, status, match, ignored, clean, unknown)
        if not match.always():
            return func()
        if ignored or clean or unknown:
            return func()
        if self._node is None or other._node is None:
            return func()
        key = 'cca.hg.buildstatus:%s:%s:v%s' % (
                node.hex(self._node), node.hex(other._node), version)
        return _mcmemoize(func, key, buildstatusserializer, ui)

    return buildstatus

def _mcmemoize(func, key, serializer, ui):
    value = None
    try:
        mcval = mcget(key, ui)
        if mcval is not None:
            ui.debug('got value for key %s from memcache\n' % key)
            value = serializer.deserialize(mcval)
            return value
    except Exception, inst:
        ui.debug('error getting or deserializing key %s: %s\n' % (key, inst))

    ui.debug('falling back for value %s from memcache\n' % key)
    value = func()

    try:
        mcset(key, serializer.serialize(value), ui)
        ui.debug('set value for key %s to memcache\n' % key)
    except Exception, inst:
        ui.debug('error setting key %s: %s\n' % (key, inst))

    return value
