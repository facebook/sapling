# sparse.py - allow sparse checkouts of the working directory
#
# Copyright 2014 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""allow sparse checkouts of the working directory (EXPERIMENTAL)

(This extension is not yet protected by backwards compatibility
guarantees. Any aspect may break in future releases until this
notice is removed.)

This extension allows the working directory to only consist of a
subset of files for the revision. This allows specific files or
directories to be explicitly included or excluded. Many repository
operations have performance proportional to the number of files in
the working directory. So only realizing a subset of files in the
working directory can improve performance.

Sparse Config Files
-------------------

The set of files that are part of a sparse checkout are defined by
a sparse config file. The file defines 3 things: includes (files to
include in the sparse checkout), excludes (files to exclude from the
sparse checkout), and profiles (links to other config files).

The file format is newline delimited. Empty lines and lines beginning
with ``#`` are ignored.

Lines beginning with ``%include `` denote another sparse config file
to include. e.g. ``%include tests.sparse``. The filename is relative
to the repository root.

The special lines ``[include]`` and ``[exclude]`` denote the section
for includes and excludes that follow, respectively. It is illegal to
have ``[include]`` after ``[exclude]``.

Non-special lines resemble file patterns to be added to either includes
or excludes. The syntax of these lines is documented by :hg:`help patterns`.
Patterns are interpreted as ``glob:`` by default and match against the
root of the repository.

Exclusion patterns take precedence over inclusion patterns. So even
if a file is explicitly included, an ``[exclude]`` entry can remove it.

For example, say you have a repository with 3 directories, ``frontend/``,
``backend/``, and ``tools/``. ``frontend/`` and ``backend/`` correspond
to different projects and it is uncommon for someone working on one
to need the files for the other. But ``tools/`` contains files shared
between both projects. Your sparse config files may resemble::

  # frontend.sparse
  frontend/**
  tools/**

  # backend.sparse
  backend/**
  tools/**

Say the backend grows in size. Or there's a directory with thousands
of files you wish to exclude. You can modify the profile to exclude
certain files::

  [include]
  backend/**
  tools/**

  [exclude]
  tools/tests/**
"""

from __future__ import absolute_import

from mercurial.i18n import _
from mercurial import (
    cmdutil,
    commands,
    dirstate,
    error,
    extensions,
    hg,
    match as matchmod,
    pycompat,
    registrar,
    sparse,
    util,
)

# Note for extension authors: ONLY specify testedwith = 'ships-with-hg-core' for
# extensions which SHIP WITH MERCURIAL. Non-mainline extensions should
# be specifying the version(s) of Mercurial they are tested with, or
# leave the attribute unspecified.
testedwith = 'ships-with-hg-core'

cmdtable = {}
command = registrar.command(cmdtable)

def extsetup(ui):
    sparse.enabled = True

    _setupclone(ui)
    _setuplog(ui)
    _setupadd(ui)
    _setupdirstate(ui)

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

def _setuplog(ui):
    entry = commands.table['^log|history']
    entry[1].append(('', 'sparse', None,
        "limit to changesets affecting the sparse checkout"))

    def _logrevs(orig, repo, opts):
        revs = orig(repo, opts)
        if opts.get('sparse'):
            sparsematch = sparse.matcher(repo)
            def ctxmatch(rev):
                ctx = repo[rev]
                return any(f for f in ctx.files() if sparsematch(f))
            revs = revs.filter(ctxmatch)
        return revs
    extensions.wrapfunction(cmdutil, '_logrevs', _logrevs)

def _clonesparsecmd(orig, ui, repo, *args, **opts):
    include_pat = opts.get('include')
    exclude_pat = opts.get('exclude')
    enableprofile_pat = opts.get('enable_profile')
    include = exclude = enableprofile = False
    if include_pat:
        pat = include_pat
        include = True
    if exclude_pat:
        pat = exclude_pat
        exclude = True
    if enableprofile_pat:
        pat = enableprofile_pat
        enableprofile = True
    if sum([include, exclude, enableprofile]) > 1:
        raise error.Abort(_("too many flags specified."))
    if include or exclude or enableprofile:
        def clonesparse(orig, self, node, overwrite, *args, **kwargs):
            sparse.updateconfig(self.unfiltered(), pat, {}, include=include,
                                exclude=exclude, enableprofile=enableprofile,
                                usereporootpaths=True)
            return orig(self, node, overwrite, *args, **kwargs)
        extensions.wrapfunction(hg, 'updaterepo', clonesparse)
    return orig(ui, repo, *args, **opts)

def _setupclone(ui):
    entry = commands.table['^clone']
    entry[1].append(('', 'enable-profile', [],
                    'enable a sparse profile'))
    entry[1].append(('', 'include', [],
                    'include sparse pattern'))
    entry[1].append(('', 'exclude', [],
                    'exclude sparse pattern'))
    extensions.wrapcommand(commands.table, 'clone', _clonesparsecmd)

def _setupadd(ui):
    entry = commands.table['^add']
    entry[1].append(('s', 'sparse', None,
                    'also include directories of added files in sparse config'))

    def _add(orig, ui, repo, *pats, **opts):
        if opts.get('sparse'):
            dirs = set()
            for pat in pats:
                dirname, basename = util.split(pat)
                dirs.add(dirname)
            sparse.updateconfig(repo, list(dirs), opts, include=True)
        return orig(ui, repo, *pats, **opts)

    extensions.wrapcommand(commands.table, 'add', _add)

def _setupdirstate(ui):
    """Modify the dirstate to prevent stat'ing excluded files,
    and to prevent modifications to files outside the checkout.
    """

    def walk(orig, self, match, subrepos, unknown, ignored, full=True):
        match = matchmod.intersectmatchers(match, self._sparsematcher)
        return orig(self, match, subrepos, unknown, ignored, full)

    extensions.wrapfunction(dirstate.dirstate, 'walk', walk)

    # dirstate.rebuild should not add non-matching files
    def _rebuild(orig, self, parent, allfiles, changedfiles=None):
        matcher = self._sparsematcher
        if not matcher.always():
            allfiles = allfiles.matches(matcher)
            if changedfiles:
                changedfiles = [f for f in changedfiles if matcher(f)]

            if changedfiles is not None:
                # In _rebuild, these files will be deleted from the dirstate
                # when they are not found to be in allfiles
                dirstatefilestoremove = set(f for f in self if not matcher(f))
                changedfiles = dirstatefilestoremove.union(changedfiles)

        return orig(self, parent, allfiles, changedfiles)
    extensions.wrapfunction(dirstate.dirstate, 'rebuild', _rebuild)

    # Prevent adding files that are outside the sparse checkout
    editfuncs = ['normal', 'add', 'normallookup', 'copy', 'remove', 'merge']
    hint = _('include file with `hg debugsparse --include <pattern>` or use ' +
             '`hg add -s <file>` to include file directory while adding')
    for func in editfuncs:
        def _wrapper(orig, self, *args):
            sparsematch = self._sparsematcher
            if not sparsematch.always():
                for f in args:
                    if (f is not None and not sparsematch(f) and
                        f not in self):
                        raise error.Abort(_("cannot add '%s' - it is outside "
                                            "the sparse checkout") % f,
                                          hint=hint)
            return orig(self, *args)
        extensions.wrapfunction(dirstate.dirstate, func, _wrapper)

@command('^debugsparse', [
    ('I', 'include', False, _('include files in the sparse checkout')),
    ('X', 'exclude', False, _('exclude files in the sparse checkout')),
    ('d', 'delete', False, _('delete an include/exclude rule')),
    ('f', 'force', False, _('allow changing rules even with pending changes')),
    ('', 'enable-profile', False, _('enables the specified profile')),
    ('', 'disable-profile', False, _('disables the specified profile')),
    ('', 'import-rules', False, _('imports rules from a file')),
    ('', 'clear-rules', False, _('clears local include/exclude rules')),
    ('', 'refresh', False, _('updates the working after sparseness changes')),
    ('', 'reset', False, _('makes the repo full again')),
    ] + commands.templateopts,
    _('[--OPTION] PATTERN...'))
def debugsparse(ui, repo, *pats, **opts):
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
    changeset is checked out. Changes to .hgsparse are not applied until they
    have been committed.

    --import-rules accepts a path to a file containing rules in the .hgsparse
    format, allowing you to add --include, --exclude and --enable-profile rules
    in bulk. Like the --include, --exclude and --enable-profile switches, the
    changes are applied immediately.

    --clear-rules removes all local include and exclude rules, while leaving
    any enabled profiles in place.

    Returns 0 if editing the sparse checkout succeeds.
    """
    opts = pycompat.byteskwargs(opts)
    include = opts.get('include')
    exclude = opts.get('exclude')
    force = opts.get('force')
    enableprofile = opts.get('enable_profile')
    disableprofile = opts.get('disable_profile')
    importrules = opts.get('import_rules')
    clearrules = opts.get('clear_rules')
    delete = opts.get('delete')
    refresh = opts.get('refresh')
    reset = opts.get('reset')
    count = sum([include, exclude, enableprofile, disableprofile, delete,
                 importrules, refresh, clearrules, reset])
    if count > 1:
        raise error.Abort(_("too many flags specified"))

    if count == 0:
        if repo.vfs.exists('sparse'):
            ui.status(repo.vfs.read("sparse") + "\n")
            temporaryincludes = sparse.readtemporaryincludes(repo)
            if temporaryincludes:
                ui.status(_("Temporarily Included Files (for merge/rebase):\n"))
                ui.status(("\n".join(temporaryincludes) + "\n"))
        else:
            ui.status(_('repo is not sparse\n'))
        return

    if include or exclude or delete or reset or enableprofile or disableprofile:
        sparse.updateconfig(repo, pats, opts, include=include, exclude=exclude,
                            reset=reset, delete=delete,
                            enableprofile=enableprofile,
                            disableprofile=disableprofile, force=force)

    if importrules:
        sparse.importfromfiles(repo, opts, pats, force=force)

    if clearrules:
        sparse.clearrules(repo, force=force)

    if refresh:
        try:
            wlock = repo.wlock()
            fcounts = map(
                len,
                sparse.refreshwdir(repo, repo.status(), sparse.matcher(repo),
                                   force=force))
            sparse.printchanges(ui, opts, added=fcounts[0], dropped=fcounts[1],
                                conflicting=fcounts[2])
        finally:
            wlock.release()
