# repository.py - Interfaces and base classes for repositories and peers.
#
# Copyright 2017 Gregory Szorc <gregory.szorc@gmail.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import abc

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

class peer(_basepeer):
    """Unified interface and base class for peer repositories.

    All peer instances must inherit from this class.
    """
