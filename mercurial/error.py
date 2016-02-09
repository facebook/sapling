# error.py - Mercurial exceptions
#
# Copyright 2005-2008 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""Mercurial exceptions.

This allows us to catch exceptions at higher levels without forcing
imports.
"""

from __future__ import absolute_import

# Do not import anything here, please

class HintException(Exception):
    def __init__(self, *args, **kw):
        Exception.__init__(self, *args)
        self.hint = kw.get('hint')

class RevlogError(HintException):
    pass

class FilteredIndexError(IndexError):
    pass

class LookupError(RevlogError, KeyError):
    def __init__(self, name, index, message):
        self.name = name
        self.index = index
        # this can't be called 'message' because at least some installs of
        # Python 2.6+ complain about the 'message' property being deprecated
        self.lookupmessage = message
        if isinstance(name, str) and len(name) == 20:
            from .node import short
            name = short(name)
        RevlogError.__init__(self, '%s@%s: %s' % (index, name, message))

    def __str__(self):
        return RevlogError.__str__(self)

class FilteredLookupError(LookupError):
    pass

class ManifestLookupError(LookupError):
    pass

class CommandError(Exception):
    """Exception raised on errors in parsing the command line."""

class InterventionRequired(HintException):
    """Exception raised when a command requires human intervention."""

class Abort(HintException):
    """Raised if a command needs to print an error and exit."""

class HookLoadError(Abort):
    """raised when loading a hook fails, aborting an operation

    Exists to allow more specialized catching."""

class HookAbort(Abort):
    """raised when a validation hook fails, aborting an operation

    Exists to allow more specialized catching."""

class ConfigError(Abort):
    """Exception raised when parsing config files"""

class UpdateAbort(Abort):
    """Raised when an update is aborted for destination issue"""

class MergeDestAbort(Abort):
    """Raised when an update is aborted for destination issues"""

class NoMergeDestAbort(MergeDestAbort):
    """Raised when an update is aborted because there is nothing to merge"""

class ManyMergeDestAbort(MergeDestAbort):
    """Raised when an update is aborted because destination is ambigious"""

class ResponseExpected(Abort):
    """Raised when an EOF is received for a prompt"""
    def __init__(self):
        from .i18n import _
        Abort.__init__(self, _('response expected'))

class OutOfBandError(HintException):
    """Exception raised when a remote repo reports failure"""

class ParseError(HintException):
    """Raised when parsing config files and {rev,file}sets (msg[, pos])"""

class UnknownIdentifier(ParseError):
    """Exception raised when a {rev,file}set references an unknown identifier"""

    def __init__(self, function, symbols):
        from .i18n import _
        ParseError.__init__(self, _("unknown identifier: %s") % function)
        self.function = function
        self.symbols = symbols

class RepoError(HintException):
    pass

class RepoLookupError(RepoError):
    pass

class FilteredRepoLookupError(RepoLookupError):
    pass

class CapabilityError(RepoError):
    pass

class RequirementError(RepoError):
    """Exception raised if .hg/requires has an unknown entry."""

class UnsupportedMergeRecords(Abort):
    def __init__(self, recordtypes):
        from .i18n import _
        self.recordtypes = sorted(recordtypes)
        s = ' '.join(self.recordtypes)
        Abort.__init__(
            self, _('unsupported merge state records: %s') % s,
            hint=_('see https://mercurial-scm.org/wiki/MergeStateRecords for '
                   'more information'))

class LockError(IOError):
    def __init__(self, errno, strerror, filename, desc):
        IOError.__init__(self, errno, strerror, filename)
        self.desc = desc

class LockHeld(LockError):
    def __init__(self, errno, filename, desc, locker):
        LockError.__init__(self, errno, 'Lock held', filename, desc)
        self.locker = locker

class LockUnavailable(LockError):
    pass

# LockError is for errors while acquiring the lock -- this is unrelated
class LockInheritanceContractViolation(RuntimeError):
    pass

class ResponseError(Exception):
    """Raised to print an error with part of output and exit."""

class UnknownCommand(Exception):
    """Exception raised if command is not in the command table."""

class AmbiguousCommand(Exception):
    """Exception raised if command shortcut matches more than one command."""

# derived from KeyboardInterrupt to simplify some breakout code
class SignalInterrupt(KeyboardInterrupt):
    """Exception raised on SIGTERM and SIGHUP."""

class SignatureError(Exception):
    pass

class PushRaced(RuntimeError):
    """An exception raised during unbundling that indicate a push race"""

# bundle2 related errors
class BundleValueError(ValueError):
    """error raised when bundle2 cannot be processed"""

class BundleUnknownFeatureError(BundleValueError):
    def __init__(self, parttype=None, params=(), values=()):
        self.parttype = parttype
        self.params = params
        self.values = values
        if self.parttype is None:
            msg = 'Stream Parameter'
        else:
            msg = parttype
        entries = self.params
        if self.params and self.values:
            assert len(self.params) == len(self.values)
            entries = []
            for idx, par in enumerate(self.params):
                val = self.values[idx]
                if val is None:
                    entries.append(val)
                else:
                    entries.append("%s=%r" % (par, val))
        if entries:
            msg = '%s - %s' % (msg, ', '.join(entries))
        ValueError.__init__(self, msg)

class ReadOnlyPartError(RuntimeError):
    """error raised when code tries to alter a part being generated"""

class PushkeyFailed(Abort):
    """error raised when a pushkey part failed to update a value"""

    def __init__(self, partid, namespace=None, key=None, new=None, old=None,
                 ret=None):
        self.partid = partid
        self.namespace = namespace
        self.key = key
        self.new = new
        self.old = old
        self.ret = ret
        # no i18n expected to be processed into a better message
        Abort.__init__(self, 'failed to update value for "%s/%s"'
                       % (namespace, key))

class CensoredNodeError(RevlogError):
    """error raised when content verification fails on a censored node

    Also contains the tombstone data substituted for the uncensored data.
    """

    def __init__(self, filename, node, tombstone):
        from .node import short
        RevlogError.__init__(self, '%s:%s' % (filename, short(node)))
        self.tombstone = tombstone

class CensoredBaseError(RevlogError):
    """error raised when a delta is rejected because its base is censored

    A delta based on a censored revision must be formed as single patch
    operation which replaces the entire base with new content. This ensures
    the delta may be applied by clones which have not censored the base.
    """

class InvalidBundleSpecification(Exception):
    """error raised when a bundle specification is invalid.

    This is used for syntax errors as opposed to support errors.
    """

class UnsupportedBundleSpecification(Exception):
    """error raised when a bundle specification is not supported."""
