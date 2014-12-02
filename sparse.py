# sparse.py - allow sparse checkouts of the working directory
#
# Copyright 2014 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""allow sparse checkouts of the working directory
"""

from mercurial import util, cmdutil, extensions, context, dirstate
from mercurial import match as matchmod
from mercurial import merge as mergemod
from mercurial.node import nullid
from mercurial.i18n import _
import errno, os, re, collections

cmdtable = {}
command = cmdutil.command(cmdtable)
testedwith = 'internal'

def uisetup(ui):
    _setupupdates(ui)
    _setupcommit(ui)

def reposetup(ui, repo):
    if not util.safehasattr(repo, 'dirstate'):
        return

    _setupdirstate(ui, repo)
    _wraprepo(ui, repo)

def wrapfilecache(cls, propname, wrapper):
    """Wraps a filecache property. These can't be wrapped using the normal
    wrapfunction. This should eventually go into upstream Mercurial.
    """
    origcls = cls
    assert callable(wrapper)
    stack = [cls]
    while stack:
        cls = stack.pop()
        if propname in cls.__dict__:
            origfn = cls.__dict__[propname].func
            assert callable(origfn)
            def wrap(*args, **kwargs):
                return wrapper(origfn, *args, **kwargs)
            cls.__dict__[propname].func = wrap
            return
        # Reverse the bases, so we descend first parents first
        stack.extend(reversed(cls.__bases__))

    raise AttributeError(_("type '%s' has no property '%s'") % (origcls,
                         propname))

def replacefilecache(cls, propname, replacement):
    """Replace a filecache property with a new class. This allows changing the
    cache invalidation condition."""
    origcls = cls
    assert callable(replacement)
    while cls is not object:
        if propname in cls.__dict__:
            orig = cls.__dict__[propname]
            setattr(cls, propname, replacement(orig))
            break
        cls = cls.__bases__[0]

    if cls is object:
        raise AttributeError(_("type '%s' has no property '%s'") % (origcls,
                             propname))

def _createactionlist():
    actions = {}
    actions['a'] = []
    actions['dg'] = []
    actions['dm'] = []
    actions['dr'] = []
    actions['e'] = []
    actions['f'] = []
    actions['g'] = []
    actions['k'] = []
    actions['m'] = []
    actions['r'] = []
    actions['rd'] = []
    return actions

def _setupupdates(ui):
    def _calculateupdates(orig, repo, wctx, mctx, pas, branchmerge, force,
                          partial, mergeancestor, followcopies):
        """Filter updates to only lay out files that match the sparse rules.
        """
        actions = orig(repo, wctx, mctx, pas, branchmerge, force,
                        partial, mergeancestor, followcopies)

        if not util.safehasattr(repo, 'sparsematch'):
            return actions

        files = set()
        prunedactions = _createactionlist()
        oldrevs = [pctx.rev() for pctx in wctx.parents()]
        oldsparsematch = repo.sparsematch(*oldrevs)

        if branchmerge:
            # If we're merging, union both matches
            sparsematch = repo.sparsematch(wctx.parents()[0].rev(), mctx.rev())
        else:
            sparsematch = repo.sparsematch(mctx.rev())

        for type, typeactions in actions.iteritems():
            pactions = []
            prunedactions[type] = pactions
            for action in typeactions:
                file, args, msg = action
                files.add(file)
                if sparsematch(file):
                    pactions.append(action)
                elif type == 'm' or (branchmerge and type == 'g'):
                    raise util.Abort(_("cannot merge because %s is outside " +
                        "the sparse checkout") % file)
                else:
                    prunedactions['r'].append((file, args, msg))

        # If an active profile changed during the update, refresh the checkout.
        profiles = repo.getactiveprofiles()
        changedprofiles = profiles & files
        if changedprofiles:
            mf = mctx.manifest()
            for file in mf:
                if file not in files:
                    old = oldsparsematch(file)
                    new = sparsematch(file)
                    if not old and new:
                        flags = mf.flags(file)
                        prunedactions['g'].append((file, (flags,), ''))
                    elif old and not new:
                        prunedactions['r'].append((file, [], ''))

        return prunedactions

    extensions.wrapfunction(mergemod, 'calculateupdates', _calculateupdates)

def _setupcommit(ui):
    def _refreshoncommit(orig, self, node):
        """Refresh the checkout when commits touch .hgsparse
        """
        orig(self, node)
        repo = self._repo
        ctx = repo[node]
        _, _, profiles = repo.getsparsepatterns(ctx.rev())
        if set(profiles) & set(ctx.files()):
            origstatus = repo.status()
            origsparsematch = repo.sparsematch()
            _refresh(repo.ui, repo, origstatus, origsparsematch, True)

    extensions.wrapfunction(context.committablectx, 'markcommitted',
        _refreshoncommit)

def _setupdirstate(ui, repo):
    """Modify the dirstate to prevent stat'ing excluded files,
    and to prevent modifications to files outside the checkout.
    """

    def _dirstate(orig, repo):
        dirstate = orig(repo)
        dirstate.repo = repo
        return dirstate
    wrapfilecache(repo.__class__, 'dirstate', _dirstate)
    if 'dirstate' in repo._filecache:
        repo.dirstate.repo = repo

    # The atrocity below is needed to wrap dirstate._ignore. It is a cached
    # property, which means normal function wrapping doesn't work.
    class ignorewrapper(object):
        def __init__(self, orig):
            self.orig = orig
            self.origignore = None
            self.func = None
            self.sparsematch = None

        def __get__(self, obj, type=None):
            repo = obj.repo
            sparsematch = repo.sparsematch()
            origignore = self.orig.__get__(obj)
            if self.sparsematch != sparsematch or self.origignore != origignore:
                self.func = lambda f: origignore(f) or not sparsematch(f)
                self.sparsematch = sparsematch
                self.origignore = origignore
            return self.func

        def __set__(self, obj, value):
            return self.orig.__set__(obj, value)

        def __delete__(self, obj):
            return self.orig.__delete__(obj)

    replacefilecache(dirstate.dirstate, '_ignore', ignorewrapper)

    # Prevent adding files that are outside the sparse checkout
    editfuncs = ['normal', 'add', 'normallookup', 'copy', 'remove', 'merge']
    for func in editfuncs:
        def _wrapper(orig, self, *args):
            repo = self.repo
            dirstate = repo.dirstate
            for f in args:
                sparsematch = repo.sparsematch()
                if not sparsematch(f) and f not in dirstate:
                    raise util.Abort(_("cannot add '%s' - it is outside the " +
                        "sparse checkout") % f)
            return orig(self, *args)
        extensions.wrapfunction(dirstate.dirstate, func, _wrapper)

def _wraprepo(ui, repo):
    class SparseRepo(repo.__class__):
        def readsparseconfig(self, raw):
            """Takes a string sparse config and returns the includes,
            excludes, and profiles it specified.
            """
            includes = set()
            excludes = set()
            current = includes
            profiles = []
            for line in raw.split('\n'):
                line = line.strip()
                if line.startswith('%include '):
                    line = line[9:].strip()
                    if line:
                        profiles.append(line)
                elif line == '[include]':
                    if current != includes:
                        raise util.abort(_('.hg/sparse cannot have includes ' +
                            'after excludes'))
                    continue
                elif line == '[exclude]':
                    current = excludes
                elif line:
                    current.add(line)

            return includes, excludes, profiles

        def getsparsepatterns(self, rev):
            """Returns the include/exclude patterns specified by the
            given rev.
            """
            if not self.opener.exists('sparse'):
                return [], [], []
            if rev is None:
                raise util.Abort(_("cannot parse sparse patterns from " +
                    "working copy"))

            raw = self.opener.read('sparse')
            includes, excludes, profiles = self.readsparseconfig(raw)

            ctx = self[rev]
            if profiles:
                visited = set()
                while profiles:
                    profile = profiles.pop()
                    if profile in visited:
                        continue
                    visited.add(profile)

                    raw = repo.filectx(profile, changeid=rev).data()
                    pincludes, pexcludes, subprofs = \
                        self.readsparseconfig(raw)
                    includes.update(pincludes)
                    excludes.update(pexcludes)
                    for subprofile in subprofs:
                        profiles.append(subprofile)

                profiles = visited

            if includes:
                includes.add('.hg*')
            return includes, excludes, profiles

        def sparsematch(self, *revs):
            """Returns the sparse match function for the given revs.

            If multiple revs are specified, the match function is the union
            of all the revs.
            """
            if not revs:
                revs = [self.changelog.rev(node) for node in
                    self.dirstate.parents() if node != nullid]

            sparsepath = self.opener.join('sparse')
            try:
                mtime = os.stat(sparsepath).st_mtime
            except OSError:
                mtime = 0
            key = str(mtime) + ' '.join([str(r) for r in revs])
            result = self.sparsecache.get(key, None)
            if result:
                return result

            matchers = []
            for rev in revs:
                try:
                    includes, excludes, profiles = self.getsparsepatterns(rev)

                    if includes or excludes:
                        matchers.append(matchmod.match(self.root, '', [],
                            include=includes, exclude=excludes,
                            default='relpath'))
                except IOError:
                    pass

            result = None
            if not matchers:
                result = matchmod.always(self.root, '')
            elif len(matchers) == 1:
                result = matchers[0]
            else:
                def unionmatcher(value):
                    for match in matchers:
                        if match(value):
                            return True
                result = unionmatcher

            self.sparsecache[key] = result

            return result

        def getactiveprofiles(self):
            revs = [self.changelog.rev(node) for node in
                    self.dirstate.parents() if node != nullid]

            activeprofiles = set()
            for rev in revs:
                _, _, profiles = self.getsparsepatterns(rev)
                activeprofiles.update(profiles)

            return activeprofiles

        def writesparseconfig(self, include, exclude, profiles):
            raw = '%s[include]\n%s\n[exclude]\n%s\n' % (
                ''.join(['%%include %s\n' % p for p in profiles]),
                '\n'.join(include),
                '\n'.join(exclude))
            self.opener.write("sparse", raw)

    repo.sparsecache = {}
    repo.__class__ = SparseRepo

@command('^sparse', [
    ('I', 'include', False, _('include files in the sparse checkout')),
    ('X', 'exclude', False, _('exclude files in the sparse checkout')),
    ('d', 'delete', False, _('delete an include/exclude rule')),
    ('f', 'force', False, _('allow changing rules even with pending changes')),
    ('', 'enable-profile', False, _('enables the specified profile')),
    ('', 'disable-profile', False, _('disables the specified profile')),
    ('', 'refresh', False, _('updates the working after sparseness changes')),
    ('', 'reset', False, _('makes the repo full again')),
    ],
    _('[--OPTION] PATTERN...'))
def sparse(ui, repo, *pats, **opts):
    """make the current checkout sparse, or edit the existing checkout

    The sparse command is used to make the current checkout sparse.
    This means files that don't meet the sparse condition will not be
    written to disk, or show up in any working copy operations. It does
    not affect files in history in any way.

    Passing no arguments prints the currently applied sparse rules.

    --include and --exclude are used to add and remove files from the sparse
    checkout. The effects of adding an include or exclude rule are applied
    immediately. If applying the new rule would cause a file with pending
    changes to be added or removed, the command will fail. Pass --force to
    force a rule change even with pending changes (the changes on disk will
    be preserved).

    --delete removes an existing include/exclude rule. The effects are
    immediate.

    --refresh refreshes the files on disk based on the sparse rules. This is
    only necessary if .hg/sparse was changed by hand.

    --enable-profile and --disable-profile accept a path to a .hgsparse file.
    This allows defining sparse checkouts and tracking them inside the
    repository. This is useful for defining commonly used sparse checkouts for
    many people to use. As the profile definition changes over time, the sparse
    checkout will automatically be updated appropriately, depending on which
    commit is checked out. Changes to .hgsparse are not applied until they
    have been committed.

    Returns 0 if editting the sparse checkout succeeds.
    """
    include = opts.get('include')
    exclude = opts.get('exclude')
    force = opts.get('force')
    enableprofile = opts.get('enable_profile')
    disableprofile = opts.get('disable_profile')
    delete = opts.get('delete')
    refresh = opts.get('refresh')
    reset = opts.get('reset')
    count = sum([include, exclude, enableprofile, disableprofile, delete,
        refresh, reset])
    if count > 1:
        raise util.Abort(_("too many flags specified"))

    if count == 0:
        if repo.opener.exists('sparse'):
            ui.status(repo.opener.read("sparse") + "\n")
        else:
            ui.status(_('repo is not sparse\n'))
        return

    oldsparsematch = repo.sparsematch()

    if repo.opener.exists('sparse'):
        raw = repo.opener.read('sparse')
        oldinclude, oldexclude, oldprofiles = repo.readsparseconfig(raw)
    else:
        oldinclude = set()
        oldexclude = set()
        oldprofiles = set()

    wlock = repo.wlock()
    try:
        try:
            oldstatus = repo.status()

            edit = (include or exclude or delete or reset or
                    enableprofile or disableprofile)
            if edit:
                if reset:
                    newinclude = set()
                    newexclude = set()
                    newprofiles = set()
                else:
                    newinclude = set(oldinclude)
                    newexclude = set(oldexclude)
                    newprofiles = set(oldprofiles)

                    if include:
                        newinclude.update(pats)

                    if exclude:
                        newexclude.update(pats)

                    if enableprofile:
                        newprofiles.update(pats)

                    if disableprofile:
                        newprofiles.difference_update(pats)

                    if delete:
                        newinclude.difference_update(pats)
                        newexclude.difference_update(pats)

                repo.writesparseconfig(newinclude, newexclude, newprofiles)
                refresh = True

            if refresh:
                _refresh(ui, repo, oldstatus, oldsparsematch, force)
        except Exception:
            repo.writesparseconfig(oldinclude, oldexclude, oldprofiles)
            raise
    finally:
        wlock.release()

def _refresh(ui, repo, origstatus, origsparsematch, force):
    """Refreshes which files are on disk by comparing the old status and
    sparsematch with the new sparsematch.

    Will raise an exception if a file with pending changes is being excluded
    or included (unless force=True).
    """
    modified, added, removed, deleted, unknown, ignored, clean = origstatus

    # Verify there are no pending changes
    pending = set()
    pending.update(modified)
    pending.update(added)
    pending.update(removed)
    sparsematch = repo.sparsematch()
    abort = False
    for file in pending:
        if not sparsematch(file):
            ui.warn(_("pending changes to '%s'\n") % file)
            abort = not force
    if abort:
        raise util.Abort(_("could not update sparseness due to " +
            "pending changes"))

    # Calculate actions
    dirstate = repo.dirstate
    ctx = repo['.']
    wctx = repo[None]
    added = []
    lookup = []
    dropped = []
    mf = ctx.manifest()
    files = set(mf)

    actions = _createactionlist()
    e_actions = actions['e']
    g_actions = actions['g']
    r_actions = actions['r']

    for file in files:
        old = origsparsematch(file)
        new = sparsematch(file)
        # Add files that are newly included, or that don't exist in
        # the dirstate yet.
        if (new and not old) or (old and new and not file in dirstate):
            fl = mf.flags(file)
            if repo.wopener.exists(file):
                e_actions.append((file, (fl,), ''))
                lookup.append(file)
            else:
                g_actions.append((file, (fl,), ''))
                added.append(file)
        # Drop files that are newly excluded, or that still exist in
        # the dirstate.
        elif (old and not new) or (not old and not new and file in dirstate):
            dropped.append(file)
            if file not in pending:
                r_actions.append((file, [], ''))

    # Verify there are no pending changes in newly included files
    abort = False
    for file, args, msg in e_actions:
        ui.warn(_("pending changes to '%s'\n") % file)
        abort = not force
    if abort:
        raise util.Abort(_("cannot change sparseness due to " +
            "pending changes (delete the files or use --force " +
            "to bring them back dirty)"))

    # Check for files that were only in the dirstate.
    for file, state in dirstate.iteritems():
        if not file in files:
            old = origsparsematch(file)
            new = sparsematch(file)
            if old and not new:
                dropped.append(file)

    # Apply changes to disk
    mergemod.applyupdates(repo, actions, repo[None], repo['.'], False)

    # Fix dirstate
    for file in added:
        dirstate.normal(file)

    for file in dropped:
        dirstate.drop(file)

    for file in lookup:
        # File exists on disk, and we're bringing it back in an unknown state.
        dirstate.normallookup(file)
