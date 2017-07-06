# sparse.py - functionality for sparse checkouts
#
# Copyright 2014 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import collections
import hashlib
import os

from .i18n import _
from .node import nullid
from . import (
    error,
    match as matchmod,
    merge as mergemod,
    pycompat,
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

def configsignature(repo, includetemp=True):
    """Obtain the signature string for the current sparse configuration.

    This is used to construct a cache key for matchers.
    """
    cache = repo._sparsesignaturecache

    signature = cache.get('signature')

    if includetemp:
        tempsignature = cache.get('tempsignature')
    else:
        tempsignature = '0'

    if signature is None or (includetemp and tempsignature is None):
        signature = hashlib.sha1(repo.vfs.tryread('sparse')).hexdigest()
        cache['signature'] = signature

        if includetemp:
            raw = repo.vfs.tryread('tempsparse')
            tempsignature = hashlib.sha1(raw).hexdigest()
            cache['tempsignature'] = tempsignature

    return '%s %s' % (signature, tempsignature)

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

def prunetemporaryincludes(repo):
    if not enabled or not repo.vfs.exists('tempsparse'):
        return

    origstatus = repo.status()
    modified, added, removed, deleted, a, b, c = origstatus
    if modified or added or removed or deleted:
        # Still have pending changes. Don't bother trying to prune.
        return

    sparsematch = matcher(repo, includetemp=False)
    dirstate = repo.dirstate
    actions = []
    dropped = []
    tempincludes = readtemporaryincludes(repo)
    for file in tempincludes:
        if file in dirstate and not sparsematch(file):
            message = _('dropping temporarily included sparse files')
            actions.append((file, None, message))
            dropped.append(file)

    typeactions = collections.defaultdict(list)
    typeactions['r'] = actions
    mergemod.applyupdates(repo, typeactions, repo[None], repo['.'], False)

    # Fix dirstate
    for file in dropped:
        dirstate.drop(file)

    repo.vfs.unlink('tempsparse')
    invalidatesignaturecache(repo)
    msg = _('cleaned up %d temporarily added file(s) from the '
            'sparse checkout\n')
    repo.ui.status(msg % len(tempincludes))

def matcher(repo, revs=None, includetemp=True):
    """Obtain a matcher for sparse working directories for the given revs.

    If multiple revisions are specified, the matcher is the union of all
    revs.

    ``includetemp`` indicates whether to use the temporary sparse profile.
    """
    # If sparse isn't enabled, sparse matcher matches everything.
    if not enabled:
        return matchmod.always(repo.root, '')

    if not revs or revs == [None]:
        revs = [repo.changelog.rev(node)
                for node in repo.dirstate.parents() if node != nullid]

    signature = configsignature(repo, includetemp=includetemp)

    key = '%s %s' % (signature, ' '.join(map(pycompat.bytestr, revs)))

    result = repo._sparsematchercache.get(key)
    if result:
        return result

    matchers = []
    for rev in revs:
        try:
            includes, excludes, profiles = patternsforrev(repo, rev)

            if includes or excludes:
                # Explicitly include subdirectories of includes so
                # status will walk them down to the actual include.
                subdirs = set()
                for include in includes:
                    # TODO consider using posix path functions here so Windows
                    # \ directory separators don't come into play.
                    dirname = os.path.dirname(include)
                    # basename is used to avoid issues with absolute
                    # paths (which on Windows can include the drive).
                    while os.path.basename(dirname):
                        subdirs.add(dirname)
                        dirname = os.path.dirname(dirname)

                matcher = matchmod.match(repo.root, '', [],
                                         include=includes, exclude=excludes,
                                         default='relpath')
                if subdirs:
                    matcher = matchmod.forceincludematcher(matcher, subdirs)
                matchers.append(matcher)
        except IOError:
            pass

    if not matchers:
        result = matchmod.always(repo.root, '')
    elif len(matchers) == 1:
        result = matchers[0]
    else:
        result = matchmod.unionmatcher(matchers)

    if includetemp:
        tempincludes = readtemporaryincludes(repo)
        result = matchmod.forceincludematcher(result, tempincludes)

    repo._sparsematchercache[key] = result

    return result

def calculateupdates(orig, repo, wctx, mctx, ancestors, branchmerge, *arg,
                      **kwargs):
    """Filter updates to only lay out files that match the sparse rules.
    """
    actions, diverge, renamedelete = orig(repo, wctx, mctx, ancestors,
                                          branchmerge, *arg, **kwargs)

    oldrevs = [pctx.rev() for pctx in wctx.parents()]
    oldsparsematch = matcher(repo, oldrevs)

    if oldsparsematch.always():
        return actions, diverge, renamedelete

    files = set()
    prunedactions = {}

    if branchmerge:
        # If we're merging, use the wctx filter, since we're merging into
        # the wctx.
        sparsematch = matcher(repo, [wctx.parents()[0].rev()])
    else:
        # If we're updating, use the target context's filter, since we're
        # moving to the target context.
        sparsematch = matcher(repo, [mctx.rev()])

    temporaryfiles = []
    for file, action in actions.iteritems():
        type, args, msg = action
        files.add(file)
        if sparsematch(file):
            prunedactions[file] = action
        elif type == 'm':
            temporaryfiles.append(file)
            prunedactions[file] = action
        elif branchmerge:
            if type != 'k':
                temporaryfiles.append(file)
                prunedactions[file] = action
        elif type == 'f':
            prunedactions[file] = action
        elif file in wctx:
            prunedactions[file] = ('r', args, msg)

    if len(temporaryfiles) > 0:
        repo.ui.status(_('temporarily included %d file(s) in the sparse '
                         'checkout for merging\n') % len(temporaryfiles))
        addtemporaryincludes(repo, temporaryfiles)

        # Add the new files to the working copy so they can be merged, etc
        actions = []
        message = 'temporarily adding to sparse checkout'
        wctxmanifest = repo[None].manifest()
        for file in temporaryfiles:
            if file in wctxmanifest:
                fctx = repo[None][file]
                actions.append((file, (fctx.flags(), False), message))

        typeactions = collections.defaultdict(list)
        typeactions['g'] = actions
        mergemod.applyupdates(repo, typeactions, repo[None], repo['.'],
                              False)

        dirstate = repo.dirstate
        for file, flags, msg in actions:
            dirstate.normal(file)

    profiles = activeprofiles(repo)
    changedprofiles = profiles & files
    # If an active profile changed during the update, refresh the checkout.
    # Don't do this during a branch merge, since all incoming changes should
    # have been handled by the temporary includes above.
    if changedprofiles and not branchmerge:
        mf = mctx.manifest()
        for file in mf:
            old = oldsparsematch(file)
            new = sparsematch(file)
            if not old and new:
                flags = mf.flags(file)
                prunedactions[file] = ('g', (flags, False), '')
            elif old and not new:
                prunedactions[file] = ('r', [], '')

    return prunedactions, diverge, renamedelete
