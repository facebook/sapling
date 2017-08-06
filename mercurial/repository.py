# repository.py - Interfaces and base classes for repositories and peers.
#
# Copyright 2017 Gregory Szorc <gregory.szorc@gmail.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import abc

from .i18n import _
from . import (
    error,
)

class _basepeer(object):
    """Represents a "connection" to a repository.

    This is the base interface for representing a connection to a repository.
    It holds basic properties and methods applicable to all peer types.

    This is not a complete interface definition and should not be used
    outside of this module.
    """
    __metaclass__ = abc.ABCMeta

    @abc.abstractproperty
    def ui(self):
        """ui.ui instance."""

    @abc.abstractmethod
    def url(self):
        """Returns a URL string representing this peer.

        Currently, implementations expose the raw URL used to construct the
        instance. It may contain credentials as part of the URL. The
        expectations of the value aren't well-defined and this could lead to
        data leakage.

        TODO audit/clean consumers and more clearly define the contents of this
        value.
        """

    @abc.abstractmethod
    def local(self):
        """Returns a local repository instance.

        If the peer represents a local repository, returns an object that
        can be used to interface with it. Otherwise returns ``None``.
        """

    @abc.abstractmethod
    def peer(self):
        """Returns an object conforming to this interface.

        Most implementations will ``return self``.
        """

    @abc.abstractmethod
    def canpush(self):
        """Returns a boolean indicating if this peer can be pushed to."""

    @abc.abstractmethod
    def close(self):
        """Close the connection to this peer.

        This is called when the peer will no longer be used. Resources
        associated with the peer should be cleaned up.
        """

class _basewirecommands(object):
    """Client-side interface for communicating over the wire protocol.

    This interface is used as a gateway to the Mercurial wire protocol.
    methods commonly call wire protocol commands of the same name.
    """
    __metaclass__ = abc.ABCMeta

    @abc.abstractmethod
    def branchmap(self):
        """Obtain heads in named branches.

        Returns a dict mapping branch name to an iterable of nodes that are
        heads on that branch.
        """

    @abc.abstractmethod
    def capabilities(self):
        """Obtain capabilities of the peer.

        Returns a set of string capabilities.
        """

    @abc.abstractmethod
    def debugwireargs(self, one, two, three=None, four=None, five=None):
        """Used to facilitate debugging of arguments passed over the wire."""

    @abc.abstractmethod
    def getbundle(self, source, **kwargs):
        """Obtain remote repository data as a bundle.

        This command is how the bulk of repository data is transferred from
        the peer to the local repository

        Returns a generator of bundle data.
        """

    @abc.abstractmethod
    def heads(self):
        """Determine all known head revisions in the peer.

        Returns an iterable of binary nodes.
        """

    @abc.abstractmethod
    def known(self, nodes):
        """Determine whether multiple nodes are known.

        Accepts an iterable of nodes whose presence to check for.

        Returns an iterable of booleans indicating of the corresponding node
        at that index is known to the peer.
        """

    @abc.abstractmethod
    def listkeys(self, namespace):
        """Obtain all keys in a pushkey namespace.

        Returns an iterable of key names.
        """

    @abc.abstractmethod
    def lookup(self, key):
        """Resolve a value to a known revision.

        Returns a binary node of the resolved revision on success.
        """

    @abc.abstractmethod
    def pushkey(self, namespace, key, old, new):
        """Set a value using the ``pushkey`` protocol.

        Arguments correspond to the pushkey namespace and key to operate on and
        the old and new values for that key.

        Returns a string with the peer result. The value inside varies by the
        namespace.
        """

    @abc.abstractmethod
    def stream_out(self):
        """Obtain streaming clone data.

        Successful result should be a generator of data chunks.
        """

    @abc.abstractmethod
    def unbundle(self, bundle, heads, url):
        """Transfer repository data to the peer.

        This is how the bulk of data during a push is transferred.

        Returns the integer number of heads added to the peer.
        """

class _baselegacywirecommands(object):
    """Interface for implementing support for legacy wire protocol commands.

    Wire protocol commands transition to legacy status when they are no longer
    used by modern clients. To facilitate identifying which commands are
    legacy, the interfaces are split.
    """
    __metaclass__ = abc.ABCMeta

    @abc.abstractmethod
    def between(self, pairs):
        """Obtain nodes between pairs of nodes.

        ``pairs`` is an iterable of node pairs.

        Returns an iterable of iterables of nodes corresponding to each
        requested pair.
        """

    @abc.abstractmethod
    def branches(self, nodes):
        """Obtain ancestor changesets of specific nodes back to a branch point.

        For each requested node, the peer finds the first ancestor node that is
        a DAG root or is a merge.

        Returns an iterable of iterables with the resolved values for each node.
        """

    @abc.abstractmethod
    def changegroup(self, nodes, kind):
        """Obtain a changegroup with data for descendants of specified nodes."""

    @abc.abstractmethod
    def changegroupsubset(self, bases, heads, kind):
        pass

class peer(_basepeer, _basewirecommands):
    """Unified interface and base class for peer repositories.

    All peer instances must inherit from this class and conform to its
    interface.
    """

    @abc.abstractmethod
    def iterbatch(self):
        """Obtain an object to be used for multiple method calls.

        Various operations call several methods on peer instances. If each
        method call were performed immediately and serially, this would
        require round trips to remote peers and/or would slow down execution.

        Some peers have the ability to "batch" method calls to avoid costly
        round trips or to facilitate concurrent execution.

        This method returns an object that can be used to indicate intent to
        perform batched method calls.

        The returned object is a proxy of this peer. It intercepts calls to
        batchable methods and queues them instead of performing them
        immediately. This proxy object has a ``submit`` method that will
        perform all queued batchable method calls. A ``results()`` method
        exposes the results of queued/batched method calls. It is a generator
        of results in the order they were called.

        Not all peers or wire protocol implementations may actually batch method
        calls. However, they must all support this API.
        """

    def capable(self, name):
        """Determine support for a named capability.

        Returns ``False`` if capability not supported.

        Returns ``True`` if boolean capability is supported. Returns a string
        if capability support is non-boolean.
        """
        caps = self.capabilities()
        if name in caps:
            return True

        name = '%s=' % name
        for cap in caps:
            if cap.startswith(name):
                return cap[len(name):]

        return False

    def requirecap(self, name, purpose):
        """Require a capability to be present.

        Raises a ``CapabilityError`` if the capability isn't present.
        """
        if self.capable(name):
            return

        raise error.CapabilityError(
            _('cannot %s; remote repository does not support the %r '
              'capability') % (purpose, name))

class legacypeer(peer, _baselegacywirecommands):
    """peer but with support for legacy wire protocol commands."""
