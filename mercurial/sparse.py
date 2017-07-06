# sparse.py - functionality for sparse checkouts
#
# Copyright 2014 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import hashlib

from .i18n import _
from .node import nullid
from . import (
    error,
)

# Whether sparse features are enabled. This variable is intended to be
# temporary to facilitate porting sparse to core. It should eventually be
# a per-repo option, possibly a repo requirement.
enabled = False

def parseconfig(ui, raw):
    """Parse sparse config file content.

    Returns a tuple of includes, excludes, and profiles.
    """
    includes = set()
    excludes = set()
    current = includes
    profiles = []
    for line in raw.split('\n'):
        line = line.strip()
        if not line or line.startswith('#'):
            # empty or comment line, skip
            continue
        elif line.startswith('%include '):
            line = line[9:].strip()
            if line:
                profiles.append(line)
        elif line == '[include]':
            if current != includes:
                # TODO pass filename into this API so we can report it.
                raise error.Abort(_('sparse config cannot have includes ' +
                                    'after excludes'))
            continue
        elif line == '[exclude]':
            current = excludes
        elif line:
            if line.strip().startswith('/'):
                ui.warn(_('warning: sparse profile cannot use' +
                          ' paths starting with /, ignoring %s\n') % line)
                continue
            current.add(line)

    return includes, excludes, profiles

# Exists as separate function to facilitate monkeypatching.
def readprofile(repo, profile, changeid):
    """Resolve the raw content of a sparse profile file."""
    # TODO add some kind of cache here because this incurs a manifest
    # resolve and can be slow.
    return repo.filectx(profile, changeid=changeid).data()

def patternsforrev(repo, rev):
    """Obtain sparse checkout patterns for the given rev.

    Returns a tuple of iterables representing includes, excludes, and
    patterns.
    """
    # Feature isn't enabled. No-op.
    if not enabled:
        return set(), set(), []

    raw = repo.vfs.tryread('sparse')
    if not raw:
        return set(), set(), []

    if rev is None:
        raise error.Abort(_('cannot parse sparse patterns from working '
                            'directory'))

    includes, excludes, profiles = parseconfig(repo.ui, raw)
    ctx = repo[rev]

    if profiles:
        visited = set()
        while profiles:
            profile = profiles.pop()
            if profile in visited:
                continue

            visited.add(profile)

            try:
                raw = readprofile(repo, profile, rev)
            except error.ManifestLookupError:
                msg = (
                    "warning: sparse profile '%s' not found "
                    "in rev %s - ignoring it\n" % (profile, ctx))
                # experimental config: sparse.missingwarning
                if repo.ui.configbool(
                        'sparse', 'missingwarning', True):
                    repo.ui.warn(msg)
                else:
                    repo.ui.debug(msg)
                continue

            pincludes, pexcludes, subprofs = parseconfig(repo.ui, raw)
            includes.update(pincludes)
            excludes.update(pexcludes)
            for subprofile in subprofs:
                profiles.append(subprofile)

        profiles = visited

    if includes:
        includes.add('.hg*')

    return includes, excludes, profiles

def activeprofiles(repo):
    revs = [repo.changelog.rev(node) for node in
            repo.dirstate.parents() if node != nullid]

    profiles = set()
    for rev in revs:
        profiles.update(patternsforrev(repo, rev)[2])

    return profiles

def invalidatesignaturecache(repo):
    repo._sparsesignaturecache.clear()

def _checksum(repo, path):
    data = repo.vfs.read(path)
    return hashlib.sha1(data).hexdigest()

def configsignature(repo, includetemp=True):
    """Obtain the signature string for the current sparse configuration.

    This is used to construct a cache key for matchers.
    """
    cache = repo._sparsesignaturecache

    signature = cache.get('signature')

    if includetemp:
        tempsignature = cache.get('tempsignature')
    else:
        tempsignature = 0

    if signature is None or (includetemp and tempsignature is None):
        signature = 0
        try:
            signature = _checksum(repo, 'sparse')
        except (OSError, IOError):
            pass
        cache['signature'] = signature

        tempsignature = 0
        if includetemp:
            try:
                tempsignature = _checksum(repo, 'tempsparse')
            except (OSError, IOError):
                pass
            cache['tempsignature'] = tempsignature

    return '%s %s' % (str(signature), str(tempsignature))

def writeconfig(repo, includes, excludes, profiles):
    """Write the sparse config file given a sparse configuration."""
    with repo.vfs('sparse', 'wb') as fh:
        for p in sorted(profiles):
            fh.write('%%include %s\n' % p)

        if includes:
            fh.write('[include]\n')
            for i in sorted(includes):
                fh.write(i)
                fh.write('\n')

        if excludes:
            fh.write('[exclude]\n')
            for e in sorted(excludes):
                fh.write(e)
                fh.write('\n')

    invalidatesignaturecache(repo)

def readtemporaryincludes(repo):
    raw = repo.vfs.tryread('tempsparse')
    if not raw:
        return set()

    return set(raw.split('\n'))

def writetemporaryincludes(repo, includes):
    repo.vfs.write('tempsparse', '\n'.join(sorted(includes)))
    invalidatesignaturecache(repo)

def addtemporaryincludes(repo, additional):
    includes = readtemporaryincludes(repo)
    for i in additional:
        includes.add(i)
    writetemporaryincludes(repo, includes)
