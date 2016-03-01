# peer.py - repository base classes for mercurial
#
# Copyright 2005, 2006 Matt Mackall <mpm@selenic.com>
# Copyright 2006 Vadim Gelfer <vadim.gelfer@gmail.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from .i18n import _
from . import (
    error,
    util,
)

# abstract batching support

class future(object):
    '''placeholder for a value to be set later'''
    def set(self, value):
        if util.safehasattr(self, 'value'):
            raise error.RepoError("future is already set")
        self.value = value

class batcher(object):
    '''base class for batches of commands submittable in a single request

    All methods invoked on instances of this class are simply queued and
    return a a future for the result. Once you call submit(), all the queued
    calls are performed and the results set in their respective futures.
    '''
    def __init__(self):
        self.calls = []
    def __getattr__(self, name):
        def call(*args, **opts):
            resref = future()
            self.calls.append((name, args, opts, resref,))
            return resref
        return call
    def submit(self):
        raise NotImplementedError()

class iterbatcher(batcher):

    def submit(self):
        raise NotImplementedError()

    def results(self):
        raise NotImplementedError()

class localbatch(batcher):
    '''performs the queued calls directly'''
    def __init__(self, local):
        batcher.__init__(self)
        self.local = local
    def submit(self):
        for name, args, opts, resref in self.calls:
            resref.set(getattr(self.local, name)(*args, **opts))

class localiterbatcher(iterbatcher):
    def __init__(self, local):
        super(iterbatcher, self).__init__()
        self.local = local

    def submit(self):
        # submit for a local iter batcher is a noop
        pass

    def results(self):
        for name, args, opts, resref in self.calls:
            yield getattr(self.local, name)(*args, **opts)

def batchable(f):
    '''annotation for batchable methods

    Such methods must implement a coroutine as follows:

    @batchable
    def sample(self, one, two=None):
        # Handle locally computable results first:
        if not one:
            yield "a local result", None
        # Build list of encoded arguments suitable for your wire protocol:
        encargs = [('one', encode(one),), ('two', encode(two),)]
        # Create future for injection of encoded result:
        encresref = future()
        # Return encoded arguments and future:
        yield encargs, encresref
        # Assuming the future to be filled with the result from the batched
        # request now. Decode it:
        yield decode(encresref.value)

    The decorator returns a function which wraps this coroutine as a plain
    method, but adds the original method as an attribute called "batchable",
    which is used by remotebatch to split the call into separate encoding and
    decoding phases.
    '''
    def plain(*args, **opts):
        batchable = f(*args, **opts)
        encargsorres, encresref = batchable.next()
        if not encresref:
            return encargsorres # a local result in this case
        self = args[0]
        encresref.set(self._submitone(f.func_name, encargsorres))
        return batchable.next()
    setattr(plain, 'batchable', f)
    return plain

class peerrepository(object):

    def batch(self):
        return localbatch(self)

    def iterbatch(self):
        """Batch requests but allow iterating over the results.

        This is to allow interleaving responses with things like
        progress updates for clients.
        """
        return localiterbatcher(self)

    def capable(self, name):
        '''tell whether repo supports named capability.
        return False if not supported.
        if boolean capability, return True.
        if string capability, return string.'''
        caps = self._capabilities()
        if name in caps:
            return True
        name_eq = name + '='
        for cap in caps:
            if cap.startswith(name_eq):
                return cap[len(name_eq):]
        return False

    def requirecap(self, name, purpose):
        '''raise an exception if the given capability is not present'''
        if not self.capable(name):
            raise error.CapabilityError(
                _('cannot %s; remote repository does not '
                  'support the %r capability') % (purpose, name))

    def local(self):
        '''return peer as a localrepo, or None'''
        return None

    def peer(self):
        return self

    def canpush(self):
        return True

    def close(self):
        pass
