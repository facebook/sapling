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
    pathutil,
    pycompat,
    scmutil,
    util,
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
    profiles = set()
    current = None
    havesection = False

    for line in raw.split('\n'):
        line = line.strip()
        if not line or line.startswith('#'):
            # empty or comment line, skip
            continue
        elif line.startswith('%include '):
            line = line[9:].strip()
            if line:
                profiles.add(line)
        elif line == '[include]':
            if havesection and current != includes:
                # TODO pass filename into this API so we can report it.
                raise error.Abort(_('sparse config cannot have includes ' +
                                    'after excludes'))
            havesection = True
            current = includes
            continue
        elif line == '[exclude]':
            havesection = True
            current = excludes
        elif line:
            if current is None:
                raise error.Abort(_('sparse config entry outside of '
                                    'section: %s') % line,
                                  hint=_('add an [include] or [exclude] line '
                                         'to declare the entry type'))

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
        return set(), set(), set()

    raw = repo.vfs.tryread('sparse')
    if not raw:
        return set(), set(), set()

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
                        'sparse', 'missingwarning'):
                    repo.ui.warn(msg)
                else:
                    repo.ui.debug(msg)
                continue

            pincludes, pexcludes, subprofs = parseconfig(repo.ui, raw)
            includes.update(pincludes)
            excludes.update(pexcludes)
            profiles.update(subprofs)

        profiles = visited

    if includes:
        includes.add('.hg*')

    return includes, excludes, profiles

def activeconfig(repo):
    """Determine the active sparse config rules.

    Rules are constructed by reading the current sparse config and bringing in
    referenced profiles from parents of the working directory.
    """
    revs = [repo.changelog.rev(node) for node in
            repo.dirstate.parents() if node != nullid]

    allincludes = set()
    allexcludes = set()
    allprofiles = set()

    for rev in revs:
        includes, excludes, profiles = patternsforrev(repo, rev)
        allincludes |= includes
        allexcludes |= excludes
        allprofiles |= profiles

    return allincludes, allexcludes, allprofiles

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

    repo._sparsesignaturecache.clear()

def readtemporaryincludes(repo):
    raw = repo.vfs.tryread('tempsparse')
    if not raw:
        return set()

    return set(raw.split('\n'))

def writetemporaryincludes(repo, includes):
    repo.vfs.write('tempsparse', '\n'.join(sorted(includes)))
    repo._sparsesignaturecache.clear()

def addtemporaryincludes(repo, additional):
    includes = readtemporaryincludes(repo)
    for i in additional:
        includes.add(i)
    writetemporaryincludes(repo, includes)

def prunetemporaryincludes(repo):
    if not enabled or not repo.vfs.exists('tempsparse'):
        return

    s = repo.status()
    if s.modified or s.added or s.removed or s.deleted:
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
    repo._sparsesignaturecache.clear()
    msg = _('cleaned up %d temporarily added file(s) from the '
            'sparse checkout\n')
    repo.ui.status(msg % len(tempincludes))

def forceincludematcher(matcher, includes):
    """Returns a matcher that returns true for any of the forced includes
    before testing against the actual matcher."""
    kindpats = [('path', include, '') for include in includes]
    includematcher = matchmod.includematcher('', '', kindpats)
    return matchmod.unionmatcher([includematcher, matcher])

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
                    matcher = forceincludematcher(matcher, subdirs)
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
        result = forceincludematcher(result, tempincludes)

    repo._sparsematchercache[key] = result

    return result

def filterupdatesactions(repo, wctx, mctx, branchmerge, actions):
    """Filter updates to only lay out files that match the sparse rules."""
    if not enabled:
        return actions

    oldrevs = [pctx.rev() for pctx in wctx.parents()]
    oldsparsematch = matcher(repo, oldrevs)

    if oldsparsematch.always():
        return actions

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

    profiles = activeconfig(repo)[2]
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

    return prunedactions

def refreshwdir(repo, origstatus, origsparsematch, force=False):
    """Refreshes working directory by taking sparse config into account.

    The old status and sparse matcher is compared against the current sparse
    matcher.

    Will abort if a file with pending changes is being excluded or included
    unless ``force`` is True.
    """
    # Verify there are no pending changes
    pending = set()
    pending.update(origstatus.modified)
    pending.update(origstatus.added)
    pending.update(origstatus.removed)
    sparsematch = matcher(repo)
    abort = False

    for f in pending:
        if not sparsematch(f):
            repo.ui.warn(_("pending changes to '%s'\n") % f)
            abort = not force

    if abort:
        raise error.Abort(_('could not update sparseness due to pending '
                            'changes'))

    # Calculate actions
    dirstate = repo.dirstate
    ctx = repo['.']
    added = []
    lookup = []
    dropped = []
    mf = ctx.manifest()
    files = set(mf)

    actions = {}

    for file in files:
        old = origsparsematch(file)
        new = sparsematch(file)
        # Add files that are newly included, or that don't exist in
        # the dirstate yet.
        if (new and not old) or (old and new and not file in dirstate):
            fl = mf.flags(file)
            if repo.wvfs.exists(file):
                actions[file] = ('e', (fl,), '')
                lookup.append(file)
            else:
                actions[file] = ('g', (fl, False), '')
                added.append(file)
        # Drop files that are newly excluded, or that still exist in
        # the dirstate.
        elif (old and not new) or (not old and not new and file in dirstate):
            dropped.append(file)
            if file not in pending:
                actions[file] = ('r', [], '')

    # Verify there are no pending changes in newly included files
    abort = False
    for file in lookup:
        repo.ui.warn(_("pending changes to '%s'\n") % file)
        abort = not force
    if abort:
        raise error.Abort(_('cannot change sparseness due to pending '
                            'changes (delete the files or use '
                            '--force to bring them back dirty)'))

    # Check for files that were only in the dirstate.
    for file, state in dirstate.iteritems():
        if not file in files:
            old = origsparsematch(file)
            new = sparsematch(file)
            if old and not new:
                dropped.append(file)

    # Apply changes to disk
    typeactions = dict((m, [])
                       for m in 'a f g am cd dc r dm dg m e k p pr'.split())
    for f, (m, args, msg) in actions.iteritems():
        if m not in typeactions:
            typeactions[m] = []
        typeactions[m].append((f, args, msg))

    mergemod.applyupdates(repo, typeactions, repo[None], repo['.'], False)

    # Fix dirstate
    for file in added:
        dirstate.normal(file)

    for file in dropped:
        dirstate.drop(file)

    for file in lookup:
        # File exists on disk, and we're bringing it back in an unknown state.
        dirstate.normallookup(file)

    return added, dropped, lookup

def aftercommit(repo, node):
    """Perform actions after a working directory commit."""
    # This function is called unconditionally, even if sparse isn't
    # enabled.
    ctx = repo[node]

    profiles = patternsforrev(repo, ctx.rev())[2]

    # profiles will only have data if sparse is enabled.
    if profiles & set(ctx.files()):
        origstatus = repo.status()
        origsparsematch = matcher(repo)
        refreshwdir(repo, origstatus, origsparsematch, force=True)

    prunetemporaryincludes(repo)

def _updateconfigandrefreshwdir(repo, includes, excludes, profiles,
                                force=False, removing=False):
    """Update the sparse config and working directory state."""
    raw = repo.vfs.tryread('sparse')
    oldincludes, oldexcludes, oldprofiles = parseconfig(repo.ui, raw)

    oldstatus = repo.status()
    oldmatch = matcher(repo)
    oldrequires = set(repo.requirements)

    # TODO remove this try..except once the matcher integrates better
    # with dirstate. We currently have to write the updated config
    # because that will invalidate the matcher cache and force a
    # re-read. We ideally want to update the cached matcher on the
    # repo instance then flush the new config to disk once wdir is
    # updated. But this requires massive rework to matcher() and its
    # consumers.

    if 'exp-sparse' in oldrequires and removing:
        repo.requirements.discard('exp-sparse')
        scmutil.writerequires(repo.vfs, repo.requirements)
    elif 'exp-sparse' not in oldrequires:
        repo.requirements.add('exp-sparse')
        scmutil.writerequires(repo.vfs, repo.requirements)

    try:
        writeconfig(repo, includes, excludes, profiles)
        return refreshwdir(repo, oldstatus, oldmatch, force=force)
    except Exception:
        if repo.requirements != oldrequires:
            repo.requirements.clear()
            repo.requirements |= oldrequires
            scmutil.writerequires(repo.vfs, repo.requirements)
        writeconfig(repo, oldincludes, oldexcludes, oldprofiles)
        raise

def clearrules(repo, force=False):
    """Clears include/exclude rules from the sparse config.

    The remaining sparse config only has profiles, if defined. The working
    directory is refreshed, as needed.
    """
    with repo.wlock():
        raw = repo.vfs.tryread('sparse')
        includes, excludes, profiles = parseconfig(repo.ui, raw)

        if not includes and not excludes:
            return

        _updateconfigandrefreshwdir(repo, set(), set(), profiles, force=force)

def importfromfiles(repo, opts, paths, force=False):
    """Import sparse config rules from files.

    The updated sparse config is written out and the working directory
    is refreshed, as needed.
    """
    with repo.wlock():
        # read current configuration
        raw = repo.vfs.tryread('sparse')
        includes, excludes, profiles = parseconfig(repo.ui, raw)
        aincludes, aexcludes, aprofiles = activeconfig(repo)

        # Import rules on top; only take in rules that are not yet
        # part of the active rules.
        changed = False
        for p in paths:
            with util.posixfile(util.expandpath(p)) as fh:
                raw = fh.read()

            iincludes, iexcludes, iprofiles = parseconfig(repo.ui, raw)
            oldsize = len(includes) + len(excludes) + len(profiles)
            includes.update(iincludes - aincludes)
            excludes.update(iexcludes - aexcludes)
            profiles.update(iprofiles - aprofiles)
            if len(includes) + len(excludes) + len(profiles) > oldsize:
                changed = True

        profilecount = includecount = excludecount = 0
        fcounts = (0, 0, 0)

        if changed:
            profilecount = len(profiles - aprofiles)
            includecount = len(includes - aincludes)
            excludecount = len(excludes - aexcludes)

            fcounts = map(len, _updateconfigandrefreshwdir(
                repo, includes, excludes, profiles, force=force))

        printchanges(repo.ui, opts, profilecount, includecount, excludecount,
                     *fcounts)

def updateconfig(repo, pats, opts, include=False, exclude=False, reset=False,
                 delete=False, enableprofile=False, disableprofile=False,
                 force=False, usereporootpaths=False):
    """Perform a sparse config update.

    Only one of the actions may be performed.

    The new config is written out and a working directory refresh is performed.
    """
    with repo.wlock():
        raw = repo.vfs.tryread('sparse')
        oldinclude, oldexclude, oldprofiles = parseconfig(repo.ui, raw)

        if reset:
            newinclude = set()
            newexclude = set()
            newprofiles = set()
        else:
            newinclude = set(oldinclude)
            newexclude = set(oldexclude)
            newprofiles = set(oldprofiles)

        if any(os.path.isabs(pat) for pat in pats):
            raise error.Abort(_('paths cannot be absolute'))

        if not usereporootpaths:
            # let's treat paths as relative to cwd
            root, cwd = repo.root, repo.getcwd()
            abspats = []
            for kindpat in pats:
                kind, pat = matchmod._patsplit(kindpat, None)
                if kind in matchmod.cwdrelativepatternkinds or kind is None:
                    ap = (kind + ':' if kind else '') +\
                            pathutil.canonpath(root, cwd, pat)
                    abspats.append(ap)
                else:
                    abspats.append(kindpat)
            pats = abspats

        if include:
            newinclude.update(pats)
        elif exclude:
            newexclude.update(pats)
        elif enableprofile:
            newprofiles.update(pats)
        elif disableprofile:
            newprofiles.difference_update(pats)
        elif delete:
            newinclude.difference_update(pats)
            newexclude.difference_update(pats)

        profilecount = (len(newprofiles - oldprofiles) -
                        len(oldprofiles - newprofiles))
        includecount = (len(newinclude - oldinclude) -
                        len(oldinclude - newinclude))
        excludecount = (len(newexclude - oldexclude) -
                        len(oldexclude - newexclude))

        fcounts = map(len, _updateconfigandrefreshwdir(
            repo, newinclude, newexclude, newprofiles, force=force,
            removing=reset))

        printchanges(repo.ui, opts, profilecount, includecount,
                     excludecount, *fcounts)

def printchanges(ui, opts, profilecount=0, includecount=0, excludecount=0,
                 added=0, dropped=0, conflicting=0):
    """Print output summarizing sparse config changes."""
    with ui.formatter('sparse', opts) as fm:
        fm.startitem()
        fm.condwrite(ui.verbose, 'profiles_added', _('Profiles changed: %d\n'),
                     profilecount)
        fm.condwrite(ui.verbose, 'include_rules_added',
                     _('Include rules changed: %d\n'), includecount)
        fm.condwrite(ui.verbose, 'exclude_rules_added',
                     _('Exclude rules changed: %d\n'), excludecount)

        # In 'plain' verbose mode, mergemod.applyupdates already outputs what
        # files are added or removed outside of the templating formatter
        # framework. No point in repeating ourselves in that case.
        if not fm.isplain():
            fm.condwrite(ui.verbose, 'files_added', _('Files added: %d\n'),
                         added)
            fm.condwrite(ui.verbose, 'files_dropped', _('Files dropped: %d\n'),
                         dropped)
            fm.condwrite(ui.verbose, 'files_conflicting',
                         _('Files conflicting: %d\n'), conflicting)
