# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# sparse.py - allow sparse checkouts of the working directory

"""allow sparse checkouts of the working directory

Sparse file format
------------------

Structure
.........

Shared sparse profile files comprise of 4 sections: `%include` directives
that pull in another sparse profile, and `[metadata]`, `[include]` and
`[exclude]` sections.

Any line starting with a `;` or `#` character is a comment and is ignored.

Extending existing profiles
...........................

`%include <absolute path>` directives (one per line) let you extend as
an existing profile file, adding more include and exclude rules. Although
this directive can appear anywere in the file, it is recommended you
keep these at the top of the file.

Metadata
........

The `[metadata]` section lets you specify key-value pairs for the profile.
Anything before the first `:` or `=` is the key, everything after is the
value. Values can be extended over multiple lines by indenting additional
lines.

Only the `title`, `description` and `hidden` keys carry meaning to for
`hg sparse`, these are used in the `hg sparse list` and
`hg sparse explain` commands. Profiles with the `hidden` key (regardless
of its value) are excluded from the `hg sparse list` listing unless
the `-v` / `--verbose` switch is given.

Include and exclude rules
.........................

Each line in the `[include]` and `[exclude]` sections is treated as a
standard pattern, see :hg:`help patterns`. Exclude rules override include
rules.

Example
.......

::

  # this profile extends another profile, incorporating all its rules
  %include some/base/profile

  [metadata]
  title: This is an example sparse profile
  description: You can include as much metadata as makes sense for your
    setup, and values can extend over multiple lines.
  lorem ipsum = Keys and values are separated by a : or =
  ; hidden: the hidden key lets you mark profiles that should not
  ;  generally be discorable. The value doesn't matter, use it to motivate
  ;  why it is hidden.

  [include]
  foo/bar/baz
  bar/python_project/**/*.py

  [exclude]
  ; exclude rules override include rules, so all files with the extension
  ; .ignore are excluded from this sparse profile.
  foo/bar/baz/*.ignore

Configuration options
---------------------

The following config option defines whether sparse treats supplied
paths as relative to repo root or to the current working dir for
include and exclude options:

    [sparse]
    includereporootpaths = off

The following config option defines whether sparse treats supplied
paths as relative to repo root or to the current working dir for
enableprofile and disableprofile options:

    [sparse]
    enablereporootpaths = on

You can configure a path to find sparse profiles in; this path is
used to discover available sparse profiles. Nested directories are
reflected in the UI.

    [sparse]
    profile_directory = tools/scm/sparse

It is not set by default.

It is also possible to show hints where dirstate size is too large.

    [sparse]
    # Whether to advertise usage of the sparse profiles when the checkout size
    # is very large.
    largecheckouthint = False
    # The number of files in the checkout that constitute a "large checkout".
    largecheckoutcount = 0
"""

from __future__ import division

import collections
import functools
import hashlib
import os
import re

from edenscm.mercurial import (
    cmdutil,
    commands,
    context,
    dirstate,
    dispatch,
    error,
    extensions,
    hg,
    hintutil,
    localrepo,
    match as matchmod,
    merge as mergemod,
    minirst,
    patch,
    pathutil,
    progress,
    pycompat,
    registrar,
    scmutil,
    util,
)
from edenscm.mercurial.i18n import _
from edenscm.mercurial.node import nullid, nullrev
from edenscm.mercurial.thirdparty import attr


cmdtable = {}
command = registrar.command(cmdtable)
configtable = {}
configitem = registrar.configitem(configtable)
testedwith = "ships-with-fb-hgext"
colortable = {
    "sparse.profile.active": "brightyellow",
    "sparse.profile.included": "yellow",
    "sparse.profile.inactive": "brightblack",
    "sparse.include": "brightgreen",
    "sparse.exclude": "brightred",
}

cwdrealtivepatkinds = ("glob", "relpath")


configitem(
    "sparse",
    "largecheckouthint",
    default=False,
    alias=[("perftweaks", "largecheckouthint")],
)
configitem(
    "sparse",
    "largecheckoutcount",
    default=0,
    alias=[("perftweaks", "largecheckoutcount")],
)


def uisetup(ui):
    _setupupdates(ui)
    _setupcommit(ui)


def extsetup(ui):
    extensions.wrapfunction(dispatch, "runcommand", _trackdirstatesizes)
    extensions.wrapfunction(dispatch, "runcommand", _tracksparseprofiles)
    _setupclone(ui)
    _setuplog(ui)
    _setupadd(ui)
    _setupdirstate(ui)
    _setupdiff(ui)
    # if fsmonitor is enabled, tell it to use our hash function
    try:
        fsmonitor = extensions.find("fsmonitor")

        def _hashignore(orig, ignore):
            return _hashmatcher(ignore)

        extensions.wrapfunction(fsmonitor, "_hashignore", _hashignore)
    except KeyError:
        pass
    # do the same for hgwatchman, old name
    try:
        hgwatchman = extensions.find("hgwatchman")

        def _hashignore(orig, ignore):
            return _hashmatcher(ignore)

        extensions.wrapfunction(hgwatchman, "_hashignore", _hashignore)
    except KeyError:
        pass


def reposetup(ui, repo):
    if not repo.local():
        return

    # The sparse extension should never be enabled in Eden repositories;
    # Eden automatically only fetches the parts of the repository that are
    # actually required.
    if "eden" in repo.requirements:
        return

    _wraprepo(ui, repo)


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
        raise AttributeError(_("type '%s' has no property '%s'") % (origcls, propname))


def _checksparse(repo):
    if "eden" in repo.requirements:
        raise error.Abort(
            _(
                "You're using an Eden repo and thus don't need sparse profiles.  "
                "See https://fburl.com/new-to-eden and enjoy!"
            )
        )

    if not util.safehasattr(repo, "sparsematch"):
        raise error.Abort(_("this is not a sparse repository"))


def _hassparse(repo):
    return "eden" not in repo.requirements and util.safehasattr(repo, "sparsematch")


def _setupupdates(ui):
    def _calculateupdates(
        orig, repo, wctx, mctx, ancestors, branchmerge, *arg, **kwargs
    ):
        """Filter updates to only lay out files that match the sparse rules.
        """
        actions, diverge, renamedelete = orig(
            repo, wctx, mctx, ancestors, branchmerge, *arg, **kwargs
        )

        # If the working context is in memory (virtual), there's no need to
        # apply the user's sparse rules at all (and in fact doing so would
        # cause unexpected behavior in the real working copy).
        if not util.safehasattr(repo, "sparsematch") or wctx.isinmemory():
            return actions, diverge, renamedelete

        files = set()
        prunedactions = {}
        oldrevs = [pctx.rev() for pctx in wctx.parents()]
        oldsparsematch = repo.sparsematch(*oldrevs)

        if branchmerge:
            # If we're merging, use the wctx filter, since we're merging into
            # the wctx.
            sparsematch = repo.sparsematch(wctx.parents()[0].rev())
        else:
            # If we're updating, use the target context's filter, since we're
            # moving to the target context.
            sparsematch = repo.sparsematch(mctx.rev())

        temporaryfiles = []
        for file, action in actions.iteritems():
            type, args, msg = action
            files.add(file)
            if sparsematch(file):
                prunedactions[file] = action
            elif type == "m":
                temporaryfiles.append(file)
                prunedactions[file] = action
            elif branchmerge:
                if type != "k":
                    temporaryfiles.append(file)
                    prunedactions[file] = action
            elif type == "f":
                prunedactions[file] = action
            elif file in wctx:
                prunedactions[file] = ("r", args, msg)

        if len(temporaryfiles) > 0:
            ui.status(
                _(
                    "temporarily included %d file(s) in the sparse checkout"
                    " for merging\n"
                )
                % len(temporaryfiles)
            )
            repo.addtemporaryincludes(temporaryfiles)

            # Add the new files to the working copy so they can be merged, etc
            actions = []
            message = "temporarily adding to sparse checkout"
            wctxmanifest = repo[None].manifest()
            for file in temporaryfiles:
                if file in wctxmanifest:
                    fctx = repo[None][file]
                    actions.append((file, (fctx.flags(), False), message))

            typeactions = collections.defaultdict(list)
            typeactions["g"] = actions
            mergemod.applyupdates(repo, typeactions, repo[None], repo["."], False)

            dirstate = repo.dirstate
            for file, flags, msg in actions:
                dirstate.normal(file)

        profiles = repo.getactiveprofiles()
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
                    prunedactions[file] = ("g", (flags, False), "")
                elif old and not new:
                    prunedactions[file] = ("r", [], "")

        return prunedactions, diverge, renamedelete

    extensions.wrapfunction(mergemod, "calculateupdates", _calculateupdates)

    def _update(orig, repo, node, branchmerge, *args, **kwargs):
        results = orig(repo, node, branchmerge, *args, **kwargs)

        # If we're updating to a location, clean up any stale temporary includes
        # (ex: this happens during hg rebase --abort).
        if not branchmerge and util.safehasattr(repo, "sparsematch"):
            repo.prunetemporaryincludes()
        return results

    extensions.wrapfunction(mergemod, "update", _update)

    def _checkcollision(orig, repo, wmf, actions):
        # If disablecasecheck is on, this should be a no-op. Run orig just to
        # be safe.
        if repo.ui.configbool("perftweaks", "disablecasecheck"):
            return orig(repo, wmf, actions)

        if util.safehasattr(repo, "sparsematch"):
            # Only check for collisions on files and directories in the
            # sparse profile
            wmf = wmf.matches(repo.sparsematch())
        return orig(repo, wmf, actions)

    extensions.wrapfunction(mergemod, "_checkcollision", _checkcollision)


def _setupcommit(ui):
    def _refreshoncommit(orig, self, node):
        """Refresh the checkout when commits touch .hgsparse
        """
        orig(self, node)

        # Use unfiltered to avoid computing hidden commits
        repo = self._repo.unfiltered()

        if util.safehasattr(repo, "getsparsepatterns"):
            ctx = repo[node]
            profiles = repo.getsparsepatterns(ctx.rev()).profiles
            if set(profiles) & set(ctx.files()):
                origstatus = repo.status()
                origsparsematch = repo.sparsematch()
                _refresh(repo.ui, repo, origstatus, origsparsematch, True)

            repo.prunetemporaryincludes()

    extensions.wrapfunction(context.committablectx, "markcommitted", _refreshoncommit)


def _setuplog(ui):
    entry = commands.table["log|history"]
    entry[1].append(
        ("", "sparse", None, "limit to changesets affecting the sparse checkout")
    )

    def _logrevs(orig, repo, opts):
        revs = orig(repo, opts)
        if opts.get("sparse"):
            sparsematch = repo.sparsematch()

            def ctxmatch(rev):
                ctx = repo[rev]
                return any(f for f in ctx.files() if sparsematch(f))

            revs = revs.filter(ctxmatch)
        return revs

    extensions.wrapfunction(cmdutil, "_logrevs", _logrevs)


def _tracksparseprofiles(runcommand, lui, repo, *args):
    res = runcommand(lui, repo, *args)
    if repo is not None and repo.local():
        # config override is used to prevent duplicated "profile not found"
        # messages. This code flow is purely for hints/reporting, it makes
        # no sense for it to warn user about the profiles for the second time
        with repo.ui.configoverride({("sparse", "missingwarning"): False}):
            if util.safehasattr(repo, "getactiveprofiles"):
                profiles = repo.getactiveprofiles()
                lui.log(
                    "sparse_profiles", "", active_profiles=",".join(sorted(profiles))
                )
    return res


def _trackdirstatesizes(runcommand, lui, repo, *args):
    res = runcommand(lui, repo, *args)
    if repo is not None and repo.local():
        dirstate = repo.dirstate
        dirstatesize = None
        try:
            # Eden and flat dirstate.
            dirstatesize = len(dirstate._map._map)
        except AttributeError:
            # Treestate and treedirstate.
            dirstatesize = len(dirstate._map)
        if dirstatesize is not None:
            lui.log("dirstate_size", dirstate_size=dirstatesize)
            if (
                repo.ui.configbool("sparse", "largecheckouthint")
                and dirstatesize >= repo.ui.configint("sparse", "largecheckoutcount")
                and _hassparse(repo)
            ):
                hintutil.trigger("sparse-largecheckout", dirstatesize, repo)
    return res


def _clonesparsecmd(orig, ui, repo, *args, **opts):
    include_pat = opts.get("include")
    exclude_pat = opts.get("exclude")
    enableprofile_pat = opts.get("enable_profile")
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

        def clone_sparse(orig, self, node, overwrite, *args, **kwargs):
            # sparse clone is a special snowflake as in that case always
            # are outside of the repo's dir hierachy, yet we always want
            # to name our includes/excludes/enables using repo-root
            # relative paths
            overrides = {
                ("sparse", "includereporootpaths"): True,
                ("sparse", "enablereporootpaths"): True,
            }
            with self.ui.configoverride(overrides, "sparse"):
                _config(
                    self.ui,
                    self.unfiltered(),
                    pat,
                    {},
                    include=include,
                    exclude=exclude,
                    enableprofile=enableprofile,
                )
            ret = orig(self, node, overwrite, *args, **kwargs)
            if enableprofile:
                _checknonexistingprofiles(ui, self, pat)
            return ret

        extensions.wrapfunction(hg, "updaterepo", clone_sparse)
    return orig(ui, repo, *args, **opts)


def _setupclone(ui):
    entry = commands.table["clone"]
    entry[1].append(("", "enable-profile", [], "enable a sparse profile"))
    entry[1].append(("", "include", [], "include sparse pattern"))
    entry[1].append(("", "exclude", [], "exclude sparse pattern"))
    extensions.wrapcommand(commands.table, "clone", _clonesparsecmd)


def _setupadd(ui):
    entry = commands.table["add"]
    entry[1].append(
        (
            "s",
            "sparse",
            None,
            "also include directories of added files in sparse config",
        )
    )

    def _add(orig, ui, repo, *pats, **opts):
        if opts.get("sparse"):
            dirs = set()
            for pat in pats:
                dirname, basename = util.split(pat)
                dirs.add(dirname)
            _config(ui, repo, list(dirs), opts, include=True)
        return orig(ui, repo, *pats, **opts)

    extensions.wrapcommand(commands.table, "add", _add)


def _setupdirstate(ui):
    """Modify the dirstate to prevent stat'ing excluded files,
    and to prevent modifications to files outside the checkout.
    """

    def _dirstate(orig, repo):
        dirstate = orig(repo)
        dirstate.repo = repo
        return dirstate

    extensions.wrapfunction(localrepo.localrepository.dirstate, "func", _dirstate)

    # The atrocity below is needed to wrap dirstate._ignore. It is a cached
    # property, which means normal function wrapping doesn't work.
    class ignorewrapper(object):
        def __init__(self, orig):
            self.orig = orig
            self.origignore = None
            self.func = None
            self.sparsematch = None

        def __get__(self, obj, type=None):
            repo = obj.repo if util.safehasattr(obj, "repo") else None
            origignore = self.orig.__get__(obj)
            if repo is None or not util.safehasattr(repo, "sparsematch"):
                return origignore

            sparsematch = repo.sparsematch()
            if self.sparsematch != sparsematch or self.origignore != origignore:
                self.func = unionmatcher([origignore, negatematcher(sparsematch)])
                self.sparsematch = sparsematch
                self.origignore = origignore
            return self.func

        def __set__(self, obj, value):
            return self.orig.__set__(obj, value)

        def __delete__(self, obj):
            return self.orig.__delete__(obj)

    replacefilecache(dirstate.dirstate, "_ignore", ignorewrapper)

    # dirstate.rebuild should not add non-matching files
    def _rebuild(orig, self, parent, allfiles, changedfiles=None, exact=False):
        if exact:
            # If exact=True, files outside "changedfiles" are assumed unchanged.
            # In this case, do not check files outside sparse profile. This
            # skips O(working copy) scans, and affect absorb perf.
            return orig(self, parent, allfiles, changedfiles, exact=exact)

        if util.safehasattr(self, "repo") and util.safehasattr(
            self.repo, "sparsematch"
        ):
            with progress.spinner(ui, "applying sparse profile"):
                matcher = self.repo.sparsematch()
                allfiles = allfiles.matches(matcher)
                if changedfiles:
                    changedfiles = [f for f in changedfiles if matcher(f)]

                if changedfiles is not None:
                    # In _rebuild, these files will be deleted from the dirstate
                    # when they are not found to be in allfiles
                    # This is O(working copy) and is expensive.
                    dirstatefilestoremove = set(f for f in self if not matcher(f))
                    changedfiles = dirstatefilestoremove.union(changedfiles)

        return orig(self, parent, allfiles, changedfiles)

    extensions.wrapfunction(dirstate.dirstate, "rebuild", _rebuild)

    # Prevent adding files that are outside the sparse checkout
    editfuncs = ["normal", "add", "normallookup", "copy", "remove", "merge"]
    hint = _(
        "include file with `hg sparse include <pattern>` or use "
        + "`hg add -s <file>` to include file directory while adding"
    )
    for func in editfuncs:

        def _wrapper(orig, self, *args):
            if util.safehasattr(self, "repo"):
                repo = self.repo
                if util.safehasattr(repo, "sparsematch"):
                    dirstate = repo.dirstate
                    sparsematch = repo.sparsematch()
                    for f in args:
                        if f is not None and not sparsematch(f) and f not in dirstate:
                            raise error.Abort(
                                _("cannot add '%s' - it is outside the sparse checkout")
                                % f,
                                hint=hint,
                            )
            return orig(self, *args)

        extensions.wrapfunction(dirstate.dirstate, func, _wrapper)


def _setupdiff(ui):
    entry = commands.table["diff|d|di|dif"]
    entry[1].append(
        ("s", "sparse", None, "only show changes in files in the sparse config")
    )

    def workingfilectxdata(orig, self):
        try:
            # Try lookup working copy first.
            return orig(self)
        except IOError:
            # Then try working copy parent if the file is outside sparse.
            if util.safehasattr(self._repo, "sparsematch"):
                sparsematch = self._repo.sparsematch()
                if not sparsematch(self._path):
                    basectx = self._changectx._parents[0]
                    return basectx[self._path].data()
            raise

    extensions.wrapfunction(context.workingfilectx, "data", workingfilectxdata)

    # wrap trydiff to filter diffs if '--sparse' is set
    def trydiff(
        orig,
        repo,
        revs,
        ctx1,
        ctx2,
        modified,
        added,
        removed,
        copy,
        getfilectx,
        opts,
        losedatafn,
        prefix,
        relroot,
    ):
        sparsematch = repo.sparsematch()
        modified = filter(sparsematch, modified)
        added = filter(sparsematch, added)
        removed = filter(sparsematch, removed)
        copy = dict((d, s) for d, s in copy.items() if sparsematch(s))
        return orig(
            repo,
            revs,
            ctx1,
            ctx2,
            modified,
            added,
            removed,
            copy,
            getfilectx,
            opts,
            losedatafn,
            prefix,
            relroot,
        )

    def diff(orig, ui, repo, *pats, **opts):
        issparse = bool(opts.get("sparse"))
        if issparse:
            extensions.wrapfunction(patch, "trydiff", trydiff)
        try:
            orig(ui, repo, *pats, **opts)
        finally:
            if issparse:
                extensions.unwrapfunction(patch, "trydiff", trydiff)

    extensions.wrapcommand(commands.table, "diff", diff)


@attr.s(frozen=True, slots=True, cmp=False)
class SparseConfig(object):
    path = attr.ib()
    includes = attr.ib(convert=frozenset)
    excludes = attr.ib(convert=frozenset)
    profiles = attr.ib(convert=tuple)
    metadata = attr.ib(default=attr.Factory(dict))

    def __iter__(self):
        # The metadata field is deliberately not included
        for field in (self.includes, self.excludes, self.profiles):
            yield field


def _wraprepo(ui, repo):
    # metadata parsing expression
    metadata_key_value = re.compile(r"(?P<key>.*)\s*[:=]\s*(?P<value>.*)")

    class SparseRepo(repo.__class__):
        def readsparseconfig(self, raw, filename=None, warn=True):
            """Takes a string sparse config and returns a SparseConfig

            This object contains the includes, excludes, and profiles from the
            raw profile.

            The filename is used to report errors and warnings, unless warn is
            set to False.

            """
            filename = filename or "<sparse profile>"
            metadata = {}
            last_key = None
            includes = set()
            excludes = set()

            sections = {
                "[include]": includes,
                "[exclude]": excludes,
                "[metadata]": metadata,
            }
            current = includes  # no sections == includes

            profiles = []
            uiwarn = self.ui.warn if warn else (lambda *ignored: None)

            for i, line in enumerate(raw.splitlines(), start=1):
                stripped = line.strip()
                if not stripped or stripped.startswith(("#", ";")):
                    # empty or comment line, skip
                    continue

                if stripped.startswith("%include "):
                    # include another profile
                    stripped = stripped[9:].strip()
                    if stripped:
                        profiles.append(stripped)
                    continue

                if stripped in sections:
                    if sections[stripped] is includes and current is excludes:
                        raise error.Abort(
                            _(
                                "A sparse file cannot have includes after excludes "
                                "in %s:%i"
                            )
                            % (filename, i)
                        )
                    current = sections[stripped]
                    continue

                if current is metadata:
                    # Metadata parsing, INI-style format
                    if line.startswith((" ", "\t")):  # continuation
                        if last_key is None:
                            uiwarn(
                                _(
                                    "warning: sparse profile [metadata] section "
                                    "indented lines that do not belong to a "
                                    "multi-line entry, ignoring, in %s:%i\n"
                                )
                                % (filename, i)
                            )
                            continue
                        key, value = last_key, stripped
                    else:
                        match = metadata_key_value.match(stripped)
                        if match is None:
                            uiwarn(
                                _(
                                    "warning: sparse profile [metadata] section "
                                    "does not appear to have a valid option "
                                    "definition, ignoring, in %s:%i\n"
                                )
                                % (filename, i)
                            )
                            last_key = None
                            continue
                        key, value = (s.strip() for s in match.group("key", "value"))
                        metadata[key] = []

                    metadata[key].append(value)
                    last_key = key
                    continue

                # inclusion or exclusion line
                if stripped.startswith("/"):
                    self.ui.warn(
                        _(
                            "warning: sparse profile cannot use paths starting "
                            "with /, ignoring %s, in %s:%i\n"
                        )
                        % (line, filename, i)
                    )
                    continue
                current.add(line)

            metadata = {
                key: "\n".join(value).strip() for key, value in metadata.items()
            }
            return SparseConfig(filename, includes, excludes, profiles, metadata)

        def getsparsepatterns(self, rev, config=None):
            """Produce the full sparse config for a revision as a SparseConfig

            This includes all patterns from included profiles, transitively.

            if config is None, use the active profile, in .hg/sparse

            """
            # Use unfiltered to avoid computing hidden commits
            if rev is None:
                raise error.Abort(_("cannot parse sparse patterns from working copy"))

            repo = self.unfiltered()
            if config is None:
                if not self.localvfs.exists("sparse"):
                    return SparseConfig(None, set(), set(), [])

                raw = self.localvfs.read("sparse")
                config = self.readsparseconfig(
                    raw, filename=self.localvfs.join("sparse")
                )

            # create copies, as these datastructures are updated further on
            includes, excludes, profiles = (
                set(config.includes),
                set(config.excludes),
                list(config.profiles),
            )

            ctx = repo[rev]
            if profiles:
                visited = set()
                while profiles:
                    profile = profiles.pop()
                    if profile in visited:
                        continue
                    visited.add(profile)

                    try:
                        raw = self.getrawprofile(profile, ctx.hex())
                    except error.ManifestLookupError:
                        msg = (
                            "warning: sparse profile '%s' not found "
                            "in rev %s - ignoring it\n" % (profile, ctx)
                        )
                        # experimental config: sparse.missingwarning
                        if self.ui.configbool("sparse", "missingwarning"):
                            self.ui.warn(msg)
                        else:
                            self.ui.debug(msg)
                        continue
                    pincludes, pexcludes, subprofs = self.readsparseconfig(
                        raw, filename=profile
                    )
                    includes.update(pincludes)
                    excludes.update(pexcludes)
                    for subprofile in subprofs:
                        profiles.append(subprofile)

                profiles = visited

            if includes:
                includes.add(".hg*")
            return SparseConfig(
                "<aggregated from %s>".format(config.path), includes, excludes, profiles
            )

        def getrawprofile(self, profile, changeid):
            repo = self.unfiltered()
            try:
                simplecache = extensions.find("simplecache")

                # Use unfiltered to avoid computing hidden commits
                node = repo[changeid].hex()

                def func():
                    return repo.filectx(profile, changeid=changeid).data()

                key = "sparseprofile:%s:%s" % (profile.replace("/", "__"), node)
                return simplecache.memoize(
                    func, key, simplecache.stringserializer, self.ui
                )
            except KeyError:
                return repo.filectx(profile, changeid=changeid).data()

        def _sparsesignature(self, includetemp=True, config=None, revs=()):
            """Returns the signature string representing the contents of the
            current project sparse configuration. This can be used to cache the
            sparse matcher for a given set of revs."""
            signaturecache = self.signaturecache
            sigkey = config.path if config else ".hg/sparse"
            signature = signaturecache.get(sigkey)
            if includetemp:
                tempsignature = signaturecache.get("tempsignature")
            else:
                tempsignature = 0

            if signature is None or (includetemp and tempsignature is None):
                signature = 0
                if config is None:
                    try:
                        sparsedata = self.localvfs.read("sparse")
                        signature = hashlib.sha1(sparsedata).hexdigest()
                    except (OSError, IOError):
                        pass
                else:
                    sha1 = hashlib.sha1()
                    for r in revs:
                        try:
                            sha1.update(self.getrawprofile(config.path, r))
                            signature = sha1.hexdigest()
                        except KeyError:
                            pass
                signaturecache[sigkey] = signature

                tempsignature = 0
                if includetemp:
                    try:
                        tempsparsepath = self.localvfs.read("tempsparse")
                        tempsignature = hashlib.sha1(tempsparsepath).hexdigest()
                    except (OSError, IOError):
                        pass
                    signaturecache["tempsignature"] = tempsignature
            return "%s:%s" % (signature, tempsignature)

        def invalidatecaches(self):
            self.invalidatesignaturecache()
            return super(SparseRepo, self).invalidatecaches()

        def invalidatesignaturecache(self):
            self.signaturecache.clear()

        def sparsematch(self, *revs, **kwargs):
            """Returns the sparse match function for the given revs

            If multiple revs are specified, the match function is the union
            of all the revs.

            `includetemp` is used to indicate if the temporarily included file
            should be part of the matcher.

            `config` can be used to specify a different sparse profile
            from the default .hg/sparse active profile

            """
            return self._sparsematch_and_key(*revs, **kwargs)[0]

        def _sparsematch_and_key(self, *revs, **kwargs):
            """Implementation of sparsematch() with the cache key included.

            This lets us reuse the key elsewhere without having to hit each
            profile file twice.

            """
            if not revs or revs == (None,):
                revs = [
                    self.changelog.rev(node)
                    for node in self.dirstate.parents()
                    if node != nullid
                ]
            if not revs:
                # Need a revision to read .hg/sparse
                revs = [nullrev]

            includetemp = kwargs.get("includetemp", True)
            config = kwargs.get("config")
            signature = self._sparsesignature(
                includetemp=includetemp, config=config, revs=revs
            )

            key = "%s:%s" % (signature, ":".join([str(r) for r in revs]))

            result = self.sparsecache.get(key, None)
            if result:
                return result, key

            matchers = []
            isalways = False
            for rev in revs:
                try:
                    includes, excludes, profiles = self.getsparsepatterns(rev, config)

                    if includes or excludes:
                        matcher = matchmod.match(
                            self.root,
                            "",
                            [],
                            include=includes,
                            exclude=excludes,
                            default="relpath",
                        )
                        matchers.append(matcher)
                    else:
                        isalways = True
                except IOError:
                    pass

            if isalways:
                result = matchmod.always(self.root, "")
            else:
                result = matchmod.union(matchers, self.root, "")

            if kwargs.get("includetemp", True):
                tempincludes = self.gettemporaryincludes()
                result = forceincludematcher(result, tempincludes)

            self.sparsecache[key] = result

            return result, key

        def getactiveprofiles(self):
            # Use unfiltered to avoid computing hidden commits
            repo = self.unfiltered()
            revs = [
                repo.changelog.rev(node)
                for node in repo.dirstate.parents()
                if node != nullid
            ]

            activeprofiles = set()
            for rev in revs:
                profiles = self.getsparsepatterns(rev).profiles
                activeprofiles.update(profiles)

            return activeprofiles

        def writesparseconfig(self, include, exclude, profiles):
            raw = "%s[include]\n%s\n[exclude]\n%s\n" % (
                "".join(["%%include %s\n" % p for p in sorted(profiles)]),
                "\n".join(sorted(include)),
                "\n".join(sorted(exclude)),
            )
            self.localvfs.write("sparse", raw)
            self.invalidatesignaturecache()

        def addtemporaryincludes(self, files):
            includes = self.gettemporaryincludes()
            for file in files:
                includes.add(file)
            self._writetemporaryincludes(includes)

        def gettemporaryincludes(self):
            existingtemp = set()
            if self.localvfs.exists("tempsparse"):
                raw = self.localvfs.read("tempsparse")
                existingtemp.update(raw.split("\n"))
            return existingtemp

        def _writetemporaryincludes(self, includes):
            raw = "\n".join(sorted(includes))
            self.localvfs.write("tempsparse", raw)
            self.invalidatesignaturecache()

        def prunetemporaryincludes(self):
            if self.localvfs.exists("tempsparse"):
                origstatus = self.status()
                modified, added, removed, deleted, a, b, c = origstatus
                if modified or added or removed or deleted:
                    # Still have pending changes. Don't bother trying to prune.
                    return

                sparsematch = self.sparsematch(includetemp=False)
                dirstate = self.dirstate
                actions = []
                dropped = []
                tempincludes = self.gettemporaryincludes()
                for file in tempincludes:
                    if file in dirstate and not sparsematch(file):
                        message = "dropping temporarily included sparse files"
                        actions.append((file, None, message))
                        dropped.append(file)

                typeactions = collections.defaultdict(list)
                typeactions["r"] = actions
                mergemod.applyupdates(self, typeactions, self[None], self["."], False)

                # Fix dirstate
                for file in dropped:
                    dirstate.untrack(file)

                self.localvfs.unlink("tempsparse")
                self.invalidatesignaturecache()
                msg = _(
                    "cleaned up %d temporarily added file(s) from the "
                    "sparse checkout\n"
                )
                self.ui.status(msg % len(tempincludes))

    if "dirstate" in repo._filecache:
        repo.dirstate.repo = repo
    repo.sparsecache = {}
    repo.signaturecache = {}
    repo.__class__ = SparseRepo


# A profile is either active, inactive or included; the latter is a profile
# included (transitively) by an active profile.
PROFILE_INACTIVE, PROFILE_ACTIVE, PROFILE_INCLUDED = _profile_flags = range(3)


@attr.s(slots=True, frozen=True)
class ProfileInfo(collections.Mapping):
    path = attr.ib()
    active = attr.ib()
    _metadata = attr.ib(default=attr.Factory(dict))

    @active.validator
    def checkactive(self, attribute, value):
        if not any(value is flag for flag in _profile_flags):
            raise ValueError("Invalid active flag value")

    # Mapping methods for metadata access
    def __getitem__(self, key):
        return self._metadata[key]

    def __iter__(self):
        return iter(self._metadata)

    def __len__(self):
        return len(self._metadata)


def _discover(ui, repo, rev=None):
    """Generate a list of available profiles with metadata

    Returns a generator yielding ProfileInfo objects, paths are relative to the
    repository root, the sequence is sorted by path.

    If no sparse.profile_directory path is configured, will only
    yield active and included profiles.

    README(.*) files are filtered out.

    If rev is given, show profiles available at that revision. The working copy
    sparse configuration is ignored and no active profile information is
    made available (all profiles are marked as 'inactive').

    """
    if not rev:
        included = repo.getactiveprofiles()
        sparse = repo.localvfs.read("sparse")
        active = repo.readsparseconfig(sparse).profiles
        active = frozenset(active)
        rev = "."
    else:
        included = active = frozenset()

    profile_directory = ui.config("sparse", "profile_directory")
    available = set()
    ctx = scmutil.revsingle(repo, rev)
    if profile_directory is not None:
        if os.path.isabs(profile_directory) or profile_directory.startswith("../"):
            raise error.Abort(
                _("sparse.profile_directory must be relative to the " "repository root")
            )
        if not profile_directory.endswith("/"):
            profile_directory += "/"

        mf = ctx.manifest()

        matcher = matchmod.match(
            repo.root,
            repo.getcwd(),
            patterns=["path:" + profile_directory],
            exclude=["relglob:README.*", "relglob:README"],
        )
        available.update(mf.matches(matcher))

    # sort profiles and read profile metadata as we iterate
    for p in sorted(available | included):
        try:
            raw = repo.getrawprofile(p, ctx.hex())
        except error.ManifestLookupError:
            # ignore a missing profile; this should only happen for 'included'
            # profiles, however. repo.getactiveprofiles() above will already
            # have printed a warning about such profiles.
            if p not in included:
                raise
            continue
        md = repo.readsparseconfig(raw, filename=p).metadata
        yield ProfileInfo(
            p,
            (
                PROFILE_ACTIVE
                if p in active
                else PROFILE_INCLUDED
                if p in included
                else PROFILE_INACTIVE
            ),
            md,
        )


def _profilesizeinfo(ui, repo, *config, **kwargs):
    """Get size stats for a given set of profiles

    Returns a dictionary of config -> (count, bytes) tuples. The
    special key `None` represents the total manifest count and
    bytecount. bytes is the total size of the files.

    Note: for performance reasons we don't calculate the total repository size
    and the value for the `None` key is always set to (count, None) to reflect
    this.

    """
    try:
        cache = extensions.find("simplecache")
        cacheget = functools.partial(
            cache.cacheget, serializer=cache.jsonserializer, ui=ui
        )
        cacheset = functools.partial(
            cache.cacheset, serializer=cache.jsonserializer, ui=ui
        )
    except KeyError:
        cacheget = cacheset = lambda *args: None

    collectsize = kwargs.get("collectsize", False)

    results = {}
    matchers = {}
    to_store = {}

    rev = kwargs.get("rev", ".")
    ctx = scmutil.revsingle(repo, rev)

    templ = "sparseprofilestats:%s:{}" % util.split(repo.root)[-1]

    def _genkey(path, *parts):
        # paths need to be ascii-safe with
        path = path.replace("/", "__")
        return templ.format(":".join((path,) + parts))

    key = _genkey("unfiltered", ctx.hex())
    cached = cacheget(key)
    results[None] = cached if cached else [0, None]
    if cached is None:
        # gather complete working copy data
        matchers[None] = matchmod.always(repo.root, repo.root)
        to_store[None] = key

    for c in config:
        matcher, key = repo._sparsematch_and_key(ctx.hex(), includetemp=False, config=c)
        key = _genkey(c.path, key, str(collectsize))
        cached = cacheget(key)
        if not cached and not collectsize:
            # if not collecting the full size, but we have a cached copy
            # for a full run, use the file count from that
            cached = cacheget(_genkey(c.path, key, "True"))
            cached = cached and [cached[0], 0]
        results[c] = cached or [0, 0]
        if cached is None:
            matchers[c] = matcher
            to_store[c] = key

    if matchers:
        mf = ctx.manifest()
        if results[None][0]:
            # use cached working copy size
            totalfiles = results[None][0]
        else:
            with progress.spinner(ui, "calculating total manifest size"):
                try:
                    totalfiles = len(mf)
                except TypeError:
                    # treemanifest does not implement __len__ :-(
                    totalfiles = sum(1 for __ in mf)

        if collectsize and len(matchers) - (None in matchers):
            # we may need to prefetch file data, to calculate the size of each
            # profile
            try:
                remotefilelog = extensions.find("remotefilelog")
            except KeyError:
                pass
            else:
                if remotefilelog.shallowrepo.requirement in repo.requirements:
                    profilematchers = unionmatcher([matchers[k] for k in matchers if k])
                    repo.prefetch(repo.revs(ctx.hex()), matcher=profilematchers)

        with progress.bar(ui, _("calculating"), total=totalfiles) as prog:
            # only matchers for which there was no cache are processed
            for file in ctx.walk(unionmatcher(matchers.values())):
                prog.value += 1
                for c, matcher in matchers.items():
                    if matcher(file):
                        results[c][0] += 1
                        if collectsize and c is not None:
                            results[c][1] += ctx.filectx(file).size()

    results = {k: tuple(v) for k, v in results.items()}
    for c, key in to_store.items():
        cacheset(key, results[c])

    return results


# hints
hint = registrar.hint()


@hint("sparse-largecheckout")
def hintlargecheckout(dirstatesize, repo):
    return (
        _(
            "Your repository checkout has %s files which makes Many mercurial "
            "commands slower. Learn how to make it smaller at "
            "https://fburl.com/hgsparse"
        )
        % dirstatesize
    )


@hint("sparse-explain-verbose")
def hintexplainverbose(*profiles):
    return _(
        "use 'hg sparse explain --verbose %s' to include the total file "
        "size for a give profile"
    ) % " ".join(profiles)


@hint("sparse-list-verbose")
def hintlistverbose(profiles, filters, load_matcher):
    # move the hidden flag from the without to the with pile and count
    # the matches
    filters["with"].add("hidden")
    filters["without"].remove("hidden")
    pred = _build_profile_filter(filters, load_matcher)
    hidden_count = sum(1 for p in filter(pred, profiles))
    if hidden_count:
        return (
            _("%d hidden profiles not shown; " "add '--verbose' to include these")
            % hidden_count
        )


_deprecate = (
    lambda o, l=_("(DEPRECATED)"): (o[:3] + (" ".join([o[4], l]),) + o[4:])
    if l not in o[4]
    else l
)


@command(
    "sparse",
    [
        (
            "f",
            "force",
            False,
            _("allow changing rules even with pending changes" "(DEPRECATED)"),
        ),
        (
            "I",
            "include",
            False,
            _("include files in the sparse checkout " "(DEPRECATED)"),
        ),
        (
            "X",
            "exclude",
            False,
            _("exclude files in the sparse checkout " "(DEPRECATED)"),
        ),
        ("d", "delete", False, _("delete an include/exclude rule " "(DEPRECATED)")),
        (
            "",
            "enable-profile",
            False,
            _("enables the specified profile " "(DEPRECATED)"),
        ),
        (
            "",
            "disable-profile",
            False,
            _("disables the specified profile " "(DEPRECATED)"),
        ),
        ("", "import-rules", False, _("imports rules from a file (DEPRECATED)")),
        (
            "",
            "clear-rules",
            False,
            _("clears local include/exclude rules " "(DEPRECATED)"),
        ),
        (
            "",
            "refresh",
            False,
            _("updates the working after sparseness changes " "(DEPRECATED)"),
        ),
        ("", "reset", False, _("makes the repo full again (DEPRECATED)")),
        (
            "",
            "cwd-list",
            False,
            _("list the full contents of the current " "directory (DEPRECATED)"),
        ),
    ]
    + [_deprecate(o) for o in commands.templateopts],
    _("SUBCOMMAND ..."),
)
def sparse(ui, repo, *pats, **opts):
    """make the current checkout sparse, or edit the existing checkout

    The sparse command is used to make the current checkout sparse.
    This means files that don't meet the sparse condition will not be
    written to disk, or show up in any working copy operations. It does
    not affect files in history in any way.

    All the work is done in subcommands such as `hg sparse enableprofile`;
    passing no subcommand prints the currently applied sparse rules.

    The `include` and `exclude` subcommands are used to add and remove files
    from the sparse checkout, while delete removes an existing include/exclude
    rule.

    Sparse profiles can also be shared with other users of the repository by
    committing a file with include and exclude rules in a separate file. Use the
    `enableprofile` and `disableprofile` subcommands to enable or disable
    such profiles. Changes to shared profiles are not applied until they have
    been committed.

    See :hg:`help -e sparse` and :hg:`help sparse [subcommand]` to get
    additional information.
    """
    _checksparse(repo)

    include = opts.get("include")
    exclude = opts.get("exclude")
    force = opts.get("force")
    enableprofile = opts.get("enable_profile")
    disableprofile = opts.get("disable_profile")
    importrules = opts.get("import_rules")
    clearrules = opts.get("clear_rules")
    delete = opts.get("delete")
    refresh = opts.get("refresh")
    reset = opts.get("reset")
    cwdlist = opts.get("cwd_list")
    count = sum(
        [
            include,
            exclude,
            enableprofile,
            disableprofile,
            delete,
            importrules,
            refresh,
            clearrules,
            reset,
            cwdlist,
        ]
    )
    if count > 1:
        raise error.Abort(_("too many flags specified"))

    if count == 0:
        if repo.localvfs.exists("sparse"):
            ui.status(repo.localvfs.read("sparse") + "\n")
            temporaryincludes = repo.gettemporaryincludes()
            if temporaryincludes:
                ui.status(_("Temporarily Included Files (for merge/rebase):\n"))
                ui.status(("\n".join(temporaryincludes) + "\n"))
        else:
            ui.status(_("repo is not sparse\n"))
        return

    if include or exclude or delete or reset or enableprofile or disableprofile:
        _config(
            ui,
            repo,
            pats,
            opts,
            include=include,
            exclude=exclude,
            reset=reset,
            delete=delete,
            enableprofile=enableprofile,
            disableprofile=disableprofile,
            force=force,
        )
        if enableprofile:
            _checknonexistingprofiles(ui, repo, pats)

    if importrules:
        _import(ui, repo, pats, opts, force=force)

    if clearrules:
        _clear(ui, repo, pats, force=force)

    if refresh:
        with repo.wlock():
            c = _refresh(ui, repo, repo.status(), repo.sparsematch(), force)
            fcounts = map(len, c)
            _verbose_output(ui, opts, 0, 0, 0, *fcounts)

    if cwdlist:
        _cwdlist(repo)


subcmd = sparse.subcommand(
    categories=[
        (
            "Show information about sparse profiles",
            ["show", "list", "explain", "files"],
        ),
        ("Change which profiles are active", ["switch", "enable", "disable"]),
        (
            "Manage additional files to include or exclude",
            ["include", "uninclude", "exclude", "unexclude", "clear"],
        ),
        ("Refresh the checkout and apply sparse profile changes", ["refresh"]),
    ]
)


@subcmd("show", commands.templateopts)
def show(ui, repo, **opts):
    """show the currently enabled sparse profile"""
    _checksparse(repo)
    if not repo.localvfs.exists("sparse"):
        if not ui.plain():
            ui.status(_("No sparse profile enabled\n"))
        return

    raw = repo.localvfs.read("sparse")
    include, exclude, profiles = map(set, repo.readsparseconfig(raw))

    def getprofileinfo(profile, depth):
        """Returns a list of (depth, profilename, title) for this profile
        and all its children."""
        sc = repo.readsparseconfig(repo.getrawprofile(profile, "."))
        profileinfo = [(depth, profile, sc.metadata.get("title"))]
        for profile in sorted(sc.profiles):
            profileinfo.extend(getprofileinfo(profile, depth + 1))
        return profileinfo

    ui.pager("sparse show")
    with ui.formatter("sparse", opts) as fm:
        if profiles:
            profileinfo = []
            fm.plain(_("Enabled Profiles:\n\n"))
            profileinfo = sum((getprofileinfo(p, 0) for p in sorted(profiles)), [])
            maxwidth = max(len(name) for depth, name, title in profileinfo)
            maxdepth = max(depth for depth, name, title in profileinfo)

            for depth, name, title in profileinfo:
                fm.startitem()
                fm.data(type="profile", depth=depth)
                if depth > 0:
                    label = "sparse.profile.included"
                    fm.plain("  " * depth + "  ~ ", label=label)
                else:
                    label = "sparse.profile.active"
                    fm.plain("  * ", label=label)
                fm.write(
                    "name",
                    "%%-%ds" % (maxwidth + (maxdepth - depth) * 2),
                    name,
                    label=label,
                )
                if title:
                    fm.write("title", "  %s", title, label=label + ".title")
                fm.plain("\n")
            if include or exclude:
                fm.plain("\n")
        if include:
            fm.plain(_("Additional Included Paths:\n\n"))
            for fname in sorted(include):
                fm.startitem()
                fm.data(type="include")
                fm.write("name", "  %s\n", fname, label="sparse.include")
            if exclude:
                fm.plain("\n")
        if exclude:
            fm.plain(_("Additional Excluded Paths:\n\n"))
            for fname in sorted(exclude):
                fm.startitem()
                fm.data(type="exclude")
                fm.write("name", "  %s\n", fname, label="sparse.exclude")


def _contains_files(load_matcher, profile, files):
    matcher = load_matcher(profile)
    return all(matcher(f) for f in files)


def _build_profile_filter(filters, load_matcher):
    """Create a callable function to filter a profile, returning a boolean"""
    predicates = {
        # we need *all* fields in a with filter to be present, so with
        # should be a subset
        "with": lambda fields: fields.issubset,
        # set.isdisjoint returns true when the iterable (dictionary keys)
        # doesn't have any names in common
        "without": lambda fields: fields.isdisjoint,
        "filter": lambda field_values: lambda md: (
            # all fields to test and all values for those fields are present
            all(
                f in md and all(v in md[f].lower() for v in vs)
                for f, vs in field_values.items()
            )
        ),
        "contains_file": lambda files: lambda md: _contains_files(
            load_matcher, md["path"], files
        ),
    }
    tests = [predicates[k](v) for k, v in sorted(filters.items()) if v]
    # pass in a dictionary with all metadata and the path as an extra key
    return lambda p: all(t(dict(p, path=p.path)) for t in tests)  # dict-from-generator


@subcmd(
    "list",
    [
        (
            "r",
            "rev",
            "",
            _("explain the profile(s) against the specified revision"),
            _("REV"),
        ),
        (
            "",
            "with-field",
            [],
            _("Only show profiles that have defined the named metadata field"),
            _("FIELD"),
        ),
        (
            "",
            "without-field",
            [],
            _("Only show profiles that do have not defined the named metadata field"),
            _("FIELD"),
        ),
        (
            "",
            "filter",
            [],
            _(
                "Only show profiles that contain the given value as a substring in a "
                "specific metadata field."
            ),
            _("FIELD:VALUE"),
        ),
        (
            "",
            "contains-file",
            [],
            _("Only show profiles that would include the named file if enabled."),
            _("FILE"),
        ),
    ]
    + commands.templateopts,
    "[OPTION]...",
)
def _listprofiles(ui, repo, *pats, **opts):
    """list available sparse profiles

    Show all available sparse profiles, with the active profiles marked.

    You can filter profiles with `--with-field [FIELD]`, `--without-field
    [FIELD]`, `--filter [FIELD:VALUE]` and `--contains-file [FILENAME]`; you can
    specify these options more than once to set multiple criteria, which all
    must match for a profile to be listed. The field `path` is always available,
    and is the path of the profile file in the repository.

    `--filter` takes a fieldname and value to look for, separated by a colon.
    The field must be present in the metadata, and the value present in the
    value for that field, to match; testing is done case-insensitively.
    Multiple filters for the same fieldname are accepted and must all match;
    e.g. --filter path:foo --filter path:bar only matches profile paths with the
    substrings foo and bar both present.

    `--contains-file` takes a file path relative to the current directory. No
    check is made if the file actually exists; any profile that would include
    the file if it did exist will match.

    By default, `--without-field hidden` is implied unless you use the --verbose
    switch to include hidden profiles.

    """
    _checksparse(repo)

    rev = scmutil.revsingle(repo, opts.get("rev")).hex()
    tocanon = functools.partial(pathutil.canonpath, repo.root, repo.getcwd())
    filters = {
        "with": set(opts.get("with_field", ())),
        "without": set(opts.get("without_field", ())),
        "filter": {},  # dictionary of fieldnames to sets of values
        "contains_file": {tocanon(f) for f in opts.get("contains_file", ())},
    }
    for fieldvalue in opts.get("filter", ()):
        fieldname, __, value = fieldvalue.partition(":")
        if not value:
            raise error.Abort(_("Missing value for filter on %s") % fieldname)
        filters["filter"].setdefault(fieldname, set()).add(value.lower())

    # It's an error to put a field both in the 'with' and 'without' buckets
    if filters["with"] & filters["without"]:
        raise error.Abort(
            _(
                "You can't specify fields in both --with-field and "
                "--without-field, please use only one or the other, for "
                "%s"
            )
            % ",".join(filters["with"] & filters["without"])
        )

    if not (ui.verbose or "hidden" in filters["with"]):
        # without the -v switch, hide profiles that have 'hidden' set. Unless,
        # of course, we specifically are filtering on hidden profiles!
        filters["without"].add("hidden")

    chars = {PROFILE_INACTIVE: "", PROFILE_INCLUDED: "~", PROFILE_ACTIVE: "*"}
    labels = {
        PROFILE_INACTIVE: "inactive",
        PROFILE_INCLUDED: "included",
        PROFILE_ACTIVE: "active",
    }
    ui.pager("sparse list")
    with ui.formatter("sparse", opts) as fm:
        fm.plain("Available Profiles:\n\n")
        load_matcher = lambda p: repo.sparsematch(
            rev=rev,
            config=repo.readsparseconfig(
                repo.getrawprofile(p, rev), filename=p, warn=False
            ),
        )

        predicate = _build_profile_filter(filters, load_matcher)
        profiles = list(_discover(ui, repo, rev=opts.get("rev")))
        filtered = list(filter(predicate, profiles))
        max_width = 0
        if not filtered:
            if fm.isplain():
                ui.write_err(_("No profiles matched the filter criteria\n"))
        else:
            max_width = max(len(p.path) for p in filtered)

        for info in filtered:
            fm.startitem()
            label = "sparse.profile." + labels[info.active]
            fm.plain(" %-1s " % chars[info.active], label=label)
            fm.data(active=labels[info.active], metadata=dict(info))
            fm.write(b"path", "%-{}s".format(max_width), info.path, label=label)
            if "title" in info:
                fm.plain("  %s" % info.get("title", b""), label=label + ".title")
            fm.plain("\n")

    if not (ui.verbose or "hidden" in filters["with"]):
        hintutil.trigger("sparse-list-verbose", profiles, filters, load_matcher)


@subcmd(
    "explain",
    [
        (
            "r",
            "rev",
            "",
            _("explain the profile(s) against the specified revision"),
            _("REV"),
        )
    ]
    + commands.templateopts,
    _("[OPTION]... [PROFILE]..."),
)
def _explainprofile(ui, repo, *profiles, **opts):
    """show information about a sparse profile

    If --verbose is given, calculates the file size impact of a profile (slow).
    """
    _checksparse(repo)

    if ui.plain() and not opts.get("template"):
        hint = _("invoke with -T/--template to control output format")
        raise error.Abort(_("must specify a template in plain mode"), hint=hint)

    if not profiles:
        raise error.Abort(_("no profiles specified"))

    rev = scmutil.revrange(repo, [opts.get("rev") or "."]).last()
    if rev is None:
        raise error.Abort(_("empty revision set"))

    configs = []
    for i, p in enumerate(profiles):
        try:
            raw = repo.getrawprofile(p, rev)
        except KeyError:
            ui.warn(_("The profile %s was not found\n") % p)
            exitcode = 255
            continue
        profile = repo.readsparseconfig(raw, p)
        configs.append(profile)

    stats = _profilesizeinfo(ui, repo, *configs, rev=rev, collectsize=ui.verbose)
    filecount, totalsize = stats[None]

    exitcode = 0

    def sortedsets(d):
        return {
            k: sorted(v) if isinstance(v, collections.Set) else v for k, v in d.items()
        }

    ui.pager("sparse explain")
    with ui.formatter("sparse", opts) as fm:
        for i, profile in enumerate(configs):
            if i:
                fm.plain("\n")
            fm.startitem()

            fm.write("path", "%s\n\n", profile.path)

            pfilecount, ptotalsize = stats.get(profile, (-1, -1))
            pfileperc = 0.0
            if pfilecount > -1 and filecount > 0:
                pfileperc = (pfilecount / filecount) * 100
            profilestats = {"filecount": pfilecount, "filecountpercentage": pfileperc}
            if ptotalsize:
                profilestats["totalsize"] = ptotalsize
            fm.data(
                stats=profilestats,
                **sortedsets(attr.asdict(profile, retain_collection_types=True))
            )

            if fm.isplain():
                md = profile.metadata
                title = md.get("title", _("(untitled)"))
                lines = [minirst.section(title)]
                description = md.get("description")
                if description:
                    lines.append("%s\n\n" % description)

                if pfileperc or ptotalsize:
                    lines.append(
                        minirst.subsection(_("Size impact compared to a full checkout"))
                    )

                    if pfileperc:
                        lines.append(
                            ":file count: {:d} ({:.2f}%)\n".format(
                                pfilecount, pfileperc
                            )
                        )
                    if ptotalsize:
                        lines.append(
                            ":total size: {:s}\n".format(util.bytecount(ptotalsize))
                        )
                    lines.append("\n")

                other = md.viewkeys() - {"title", "description"}
                if other:
                    lines += (
                        minirst.subsection(_("Additional metadata")),
                        "".join(
                            [
                                ":%s: %s\n" % (key, "\n  ".join(md[key].splitlines()))
                                for key in sorted(other)
                            ]
                        ),
                        "\n",
                    )

                sections = (
                    ("profiles", _("Profiles included")),
                    ("includes", _("Inclusion rules")),
                    ("excludes", _("Exclusion rules")),
                )

                for attrib, label in sections:
                    section = getattr(profile, attrib)
                    if not section:
                        continue
                    lines += (minirst.subsection(label), "::\n\n")
                    lines += ("  %s\n" % entry for entry in sorted(section))
                    lines += ("\n",)

                textwidth = ui.configint("ui", "textwidth")
                termwidth = ui.termwidth() - 2
                if not (0 < textwidth <= termwidth):
                    textwidth = termwidth
                fm.plain(minirst.format("".join(lines), textwidth))

    if not ui.verbose:
        hintutil.trigger("sparse-explain-verbose", *profiles)

    return exitcode


@subcmd("files", commands.templateopts, _("[OPTION]... PROFILE [FILES]..."))
def _listfilessubcmd(ui, repo, profile, *files, **opts):
    """list all files included in a sparse profile

    If files are given to match, print the names of the files in the profile
    that match those patterns.

    """
    _checksparse(repo)
    try:
        raw = repo.getrawprofile(profile, ".")
    except KeyError:
        raise error.Abort(_("The profile %s was not found\n") % profile)

    config = repo.readsparseconfig(raw, profile)
    ctx = repo["."]
    matcher = matchmod.intersectmatchers(
        matchmod.match(repo.root, repo.getcwd(), files),
        repo.sparsematch(ctx.hex(), includetemp=False, config=config),
    )

    exitcode = 1
    ui.pager("sparse listfiles")
    with ui.formatter("files", opts) as fm:
        for f in ctx.matches(matcher):
            fm.startitem()
            fm.data(abspath=f)
            fm.write("path", "%s\n", matcher.rel(f))
            exitcode = 0
    return exitcode


_common_config_opts = [
    ("f", "force", False, _("allow changing rules even with pending changes"))
]


@subcmd("reset", _common_config_opts + commands.templateopts)
def resetsubcmd(ui, repo, **opts):
    """disable all sparse profiles and convert to a full checkout"""
    _config(ui, repo, [], opts, force=opts.get("force"), reset=True)


@subcmd("disable|disableprofile", _common_config_opts, "[PROFILE]...")
def disableprofilesubcmd(ui, repo, *pats, **opts):
    """disable a sparse profile"""
    _config(ui, repo, pats, opts, force=opts.get("force"), disableprofile=True)


@subcmd("enable|enableprofile", _common_config_opts, "[PROFILE]...")
def enableprofilesubcmd(ui, repo, *pats, **opts):
    """enable a sparse profile"""
    _checknonexistingprofiles(ui, repo, pats)
    _config(ui, repo, pats, opts, force=opts.get("force"), enableprofile=True)


@subcmd("switch|switchprofile", _common_config_opts, "[PROFILE]...")
def switchprofilesubcmd(ui, repo, *pats, **opts):
    """switch to another sparse profile

    Disables all other profiles and stops including and excluding any additional
    files you have previously included or excluded.
    """
    _checknonexistingprofiles(ui, repo, pats)
    _config(
        ui, repo, pats, opts, force=opts.get("force"), reset=True, enableprofile=True
    )


@subcmd("delete", _common_config_opts, "[RULE]...")
def deletesubcmd(ui, repo, *pats, **opts):
    """delete an include or exclude rule (DEPRECATED)"""
    _config(ui, repo, pats, opts, force=opts.get("force"), delete=True)


@subcmd("exclude", _common_config_opts, "[RULE]...")
def excludesubcmd(ui, repo, *pats, **opts):
    """exclude some additional files"""
    _config(ui, repo, pats, opts, force=opts.get("force"), exclude=True)


@subcmd("unexclude", _common_config_opts, "[RULE]...")
def unexcludesubcmd(ui, repo, *pats, **opts):
    """stop excluding some additional files"""
    _config(ui, repo, pats, opts, force=opts.get("force"), unexclude=True)


@subcmd("include", _common_config_opts, "[RULE]...")
def includesubcmd(ui, repo, *pats, **opts):
    """include some additional files"""
    _config(ui, repo, pats, opts, force=opts.get("force"), include=True)


@subcmd("uninclude", _common_config_opts, "[RULE]...")
def unincludesubcmd(ui, repo, *pats, **opts):
    """stop including some additional files"""
    _config(ui, repo, pats, opts, force=opts.get("force"), uninclude=True)


for c in deletesubcmd, excludesubcmd, includesubcmd, unexcludesubcmd, unincludesubcmd:
    c.__doc__ += """\n
The effects of adding or deleting an include or exclude rule are applied
immediately. If applying the new rule would cause a file with pending
changes to be added or removed, the command will fail. Pass --force to
force a rule change even with pending changes (the changes on disk will
be preserved).
"""


@subcmd("importrules", _common_config_opts, _("[OPTION]... [FILE]..."))
def _importsubcmd(ui, repo, *pats, **opts):
    """import sparse profile rules

    Accepts a path to a file containing rules in the .hgsparse format.

    This allows you to add *include*, *exclude* and *enable* rules
    in bulk. Like the include, exclude and enable subcommands, the
    changes are applied immediately.

    """
    _import(ui, repo, pats, opts, force=opts.get("force"))


@subcmd("clear", _common_config_opts, _("[OPTION]..."))
def _clearsubcmd(ui, repo, *pats, **opts):
    """clear all extra files included or excluded

    Removes all extra include and exclude rules, without changing which
    profiles are enabled.

    """
    _clear(ui, repo, pats, force=opts.get("force"))


@subcmd("refresh", _common_config_opts, _("[OPTION]..."))
def _refreshsubcmd(ui, repo, *pats, **opts):
    """refresh the files on disk based on the enabled sparse profiles

    This is only necessary if .hg/sparse was changed by hand.
    """
    _checksparse(repo)

    force = opts.get("force")
    with repo.wlock():
        c = _refresh(ui, repo, repo.status(), repo.sparsematch(), force)
        fcounts = map(len, c)
        _verbose_output(ui, opts, 0, 0, 0, *fcounts)


@subcmd("cwd")
def _cwdsubcmd(ui, repo, *pats, **opts):
    """list all names in this directory

    The list includes any names that are excluded by the current sparse
    checkout; these are annotated with a hyphen ('-') before the name.

    """
    _cwdlist(repo)


def _config(
    ui,
    repo,
    pats,
    opts,
    include=False,
    exclude=False,
    reset=False,
    delete=False,
    uninclude=False,
    unexclude=False,
    enableprofile=False,
    disableprofile=False,
    force=False,
):
    _checksparse(repo)

    """
    Perform a sparse config update. Only one of the kwargs may be specified.
    """
    wlock = repo.wlock()
    try:
        oldsparsematch = repo.sparsematch()

        if repo.localvfs.exists("sparse"):
            raw = repo.localvfs.read("sparse")
            oldinclude, oldexclude, oldprofiles = map(set, repo.readsparseconfig(raw))
        else:
            oldinclude = set()
            oldexclude = set()
            oldprofiles = set()

        try:
            if reset:
                newinclude = set()
                newexclude = set()
                newprofiles = set()
            else:
                newinclude = set(oldinclude)
                newexclude = set(oldexclude)
                newprofiles = set(oldprofiles)

            if any(os.path.isabs(pat) for pat in pats):
                err = _("paths cannot be absolute")
                raise error.Abort(err)

            adjustpats = False
            if include or exclude or delete or uninclude or unexclude:
                if not ui.configbool("sparse", "includereporootpaths", False):
                    adjustpats = True
            if enableprofile or disableprofile:
                if not ui.configbool("sparse", "enablereporootpaths", True):
                    adjustpats = True
            if adjustpats:
                # supplied file patterns should be treated as relative
                # to current working dir, so we need to convert them first
                root, cwd = repo.root, repo.getcwd()
                abspats = []
                for kindpat in pats:
                    kind, pat = matchmod._patsplit(kindpat, None)
                    if kind in cwdrealtivepatkinds or kind is None:
                        kindpat = (kind + ":" if kind else "") + pathutil.canonpath(
                            root, cwd, pat
                        )
                    abspats.append(kindpat)
                pats = abspats

            oldstatus = repo.status()
            if include:
                newexclude.difference_update(pats)
                newinclude.update(pats)
            elif exclude:
                newinclude.difference_update(pats)
                newexclude.update(pats)
            elif enableprofile:
                newprofiles.update(pats)
            elif disableprofile:
                newprofiles.difference_update(pats)
            elif uninclude:
                newinclude.difference_update(pats)
            elif unexclude:
                newexclude.difference_update(pats)
            elif delete:
                newinclude.difference_update(pats)
                newexclude.difference_update(pats)

            repo.writesparseconfig(newinclude, newexclude, newprofiles)
            fcounts = map(len, _refresh(ui, repo, oldstatus, oldsparsematch, force))

            profilecount = len(newprofiles - oldprofiles) - len(
                oldprofiles - newprofiles
            )
            includecount = len(newinclude - oldinclude) - len(oldinclude - newinclude)
            excludecount = len(newexclude - oldexclude) - len(oldexclude - newexclude)
            _verbose_output(
                ui, opts, profilecount, includecount, excludecount, *fcounts
            )
        except Exception:
            repo.writesparseconfig(oldinclude, oldexclude, oldprofiles)
            raise
    finally:
        wlock.release()


def _checknonexistingprofiles(ui, repo, profiles):
    # don't do the check if sparse.missingwarning is set to true
    # otherwise warnings will be shown twice for the same profile
    if ui.configbool("sparse", "missingwarning"):
        return

    for p in profiles:
        try:
            repo.filectx(p, changeid=".").data()
        except error.ManifestLookupError:
            ui.warn(
                _(
                    "the profile '%s' does not exist in the "
                    "current commit, it will only take effect "
                    "when you check out a commit containing a "
                    "profile with that name\n"
                )
                % p
            )


def _import(ui, repo, files, opts, force=False):
    _checksparse(repo)

    with repo.wlock():
        # load union of current active profile
        revs = [
            repo.changelog.rev(node)
            for node in repo.dirstate.parents()
            if node != nullid
        ]

        # read current configuration
        raw = ""
        if repo.localvfs.exists("sparse"):
            raw = repo.localvfs.read("sparse")
        oincludes, oexcludes, oprofiles = repo.readsparseconfig(raw)
        includes, excludes, profiles = map(set, (oincludes, oexcludes, oprofiles))

        # all active rules
        aincludes, aexcludes, aprofiles = set(), set(), set()
        for rev in revs:
            rincludes, rexcludes, rprofiles = repo.getsparsepatterns(rev)
            aincludes.update(rincludes)
            aexcludes.update(rexcludes)
            aprofiles.update(rprofiles)

        # import rules on top; only take in rules that are not yet
        # part of the active rules.
        changed = False
        for file in files:
            with util.posixfile(util.expandpath(file)) as importfile:
                iincludes, iexcludes, iprofiles = repo.readsparseconfig(
                    importfile.read(), filename=file
                )
                oldsize = len(includes) + len(excludes) + len(profiles)
                includes.update(iincludes - aincludes)
                excludes.update(iexcludes - aexcludes)
                profiles.update(set(iprofiles) - aprofiles)
                if len(includes) + len(excludes) + len(profiles) > oldsize:
                    changed = True

        profilecount = includecount = excludecount = 0
        fcounts = (0, 0, 0)

        if changed:
            profilecount = len(profiles - aprofiles)
            includecount = len(includes - aincludes)
            excludecount = len(excludes - aexcludes)

            oldstatus = repo.status()
            oldsparsematch = repo.sparsematch()
            repo.writesparseconfig(includes, excludes, profiles)

            try:
                fcounts = map(len, _refresh(ui, repo, oldstatus, oldsparsematch, force))
            except Exception:
                repo.writesparseconfig(oincludes, oexcludes, oprofiles)
                raise

        _verbose_output(ui, opts, profilecount, includecount, excludecount, *fcounts)


def _clear(ui, repo, files, force=False):
    _checksparse(repo)

    with repo.wlock():
        raw = ""
        if repo.localvfs.exists("sparse"):
            raw = repo.localvfs.read("sparse")
        includes, excludes, profiles = repo.readsparseconfig(raw)

        if includes or excludes:
            oldstatus = repo.status()
            oldsparsematch = repo.sparsematch()
            repo.writesparseconfig(set(), set(), profiles)
            _refresh(ui, repo, oldstatus, oldsparsematch, force)


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
    if len(pending) > 0:
        ui.note(_("verifying pending changes for refresh\n"))
    for file in pending:
        if not sparsematch(file):
            ui.warn(_("pending changes to '%s'\n") % file)
            abort = not force
    if abort:
        raise error.Abort(_("could not update sparseness due to pending changes"))

    # Calculate actions
    ui.note(_("calculating actions for refresh\n"))
    with progress.spinner(ui, "populating file set"):
        dirstate = repo.dirstate
        ctx = repo["."]
        added = []
        lookup = []
        dropped = []
        mf = ctx.manifest()
        files = set(mf)

    actions = {}

    with progress.bar(ui, _("calculating"), total=len(files)) as prog:
        for file in files:
            prog.value += 1

            old = origsparsematch(file)
            new = sparsematch(file)
            # Add files that are newly included, or that don't exist in
            # the dirstate yet.
            if (new and not old) or (old and new and not file in dirstate):
                fl = mf.flags(file)
                if repo.wvfs.exists(file):
                    actions[file] = ("e", (fl,), "")
                    lookup.append(file)
                else:
                    actions[file] = ("g", (fl, False), "")
                    added.append(file)
            # Drop files that are newly excluded, or that still exist in
            # the dirstate.
            elif (old and not new) or (not (old or new) and file in dirstate):
                dropped.append(file)
                if file not in pending:
                    actions[file] = ("r", [], "")

    # Verify there are no pending changes in newly included files
    if len(lookup) > 0:
        ui.note(_("verifying no pending changes in newly included files\n"))
    abort = False
    for file in lookup:
        ui.warn(_("pending changes to '%s'\n") % file)
        abort = not force
    if abort:
        raise error.Abort(
            _(
                "cannot change sparseness due to "
                + "pending changes (delete the files or use --force "
                + "to bring them back dirty)"
            )
        )

    # Check for files that were only in the dirstate.
    for file, state in dirstate.iteritems():
        if not file in files:
            old = origsparsematch(file)
            new = sparsematch(file)
            if old and not new:
                dropped.append(file)

    # Apply changes to disk
    if len(actions) > 0:
        ui.note(_("applying changes to disk (%d actions)\n") % len(actions))
    typeactions = dict((m, []) for m in "a f g am cd dc r dm dg m e k p pr".split())

    with progress.bar(ui, _("applying"), total=len(actions)) as prog:
        for f, (m, args, msg) in actions.iteritems():
            prog.value += 1
            if m not in typeactions:
                typeactions[m] = []
            typeactions[m].append((f, args, msg))
        mergemod.applyupdates(repo, typeactions, repo[None], repo["."], False)

    # Fix dirstate
    filecount = len(added) + len(dropped) + len(lookup)
    if filecount > 0:
        ui.note(_("updating dirstate\n"))
    with progress.bar(ui, _("recording"), _("files"), filecount) as prog:
        for file in added:
            prog.value += 1
            dirstate.normal(file)

        for file in dropped:
            prog.value += 1
            dirstate.untrack(file)

        for file in lookup:
            prog.value += 1
            # File exists on disk, and we're bringing it back in an unknown
            # state.
            dirstate.normallookup(file)

    return added, dropped, lookup


def _verbose_output(
    ui, opts, profilecount, includecount, excludecount, added, dropped, lookup
):
    """Produce --verbose and templatable output

    This specifically enables -Tjson, providing machine-readable stats on how
    the sparse profile changed.

    """
    with ui.formatter("sparse", opts) as fm:
        fm.startitem()
        fm.condwrite(
            ui.verbose, "profiles_added", "Profile # change: %d\n", profilecount
        )
        fm.condwrite(
            ui.verbose,
            "include_rules_added",
            "Include rule # change: %d\n",
            includecount,
        )
        fm.condwrite(
            ui.verbose,
            "exclude_rules_added",
            "Exclude rule # change: %d\n",
            excludecount,
        )
        # In 'plain' verbose mode, mergemod.applyupdates already outputs what
        # files are added or removed outside of the templating formatter
        # framework. No point in repeating ourselves in that case.
        if not fm.isplain():
            fm.condwrite(ui.verbose, "files_added", "Files added: %d\n", added)
            fm.condwrite(ui.verbose, "files_dropped", "Files dropped: %d\n", dropped)
            fm.condwrite(
                ui.verbose, "files_conflicting", "Files conflicting: %d\n", lookup
            )


def _cwdlist(repo):
    """ List the contents in the current directory. Annotate
    the files in the sparse profile.
    """
    _checksparse(repo)

    ctx = repo["."]
    mf = ctx.manifest()

    # Get the root of the repo so that we remove the content of
    # the root from the current working directory
    root = repo.root
    cwd = util.normpath(pycompat.getcwd())
    cwd = os.path.relpath(cwd, root)
    cwd = "" if cwd == os.curdir else cwd + pycompat.ossep
    if cwd.startswith(os.pardir + pycompat.ossep):
        raise error.Abort(
            _("the current working directory should begin " "with the root %s") % root
        )

    matcher = matchmod.match(repo.root, repo.getcwd(), patterns=["path:" + cwd])
    files = mf.matches(matcher)

    sparsematch = repo.sparsematch(ctx.rev())
    checkedoutentries = set()
    allentries = set()
    cwdlength = len(cwd)

    for filepath in files:
        entryname = filepath[cwdlength:].partition(pycompat.ossep)[0]

        allentries.add(entryname)
        if sparsematch(filepath):
            checkedoutentries.add(entryname)

    ui = repo.ui
    for entry in sorted(allentries):
        marker = " " if entry in checkedoutentries else "-"
        ui.status("%s %s\n" % (marker, entry))


class forceincludematcher(matchmod.basematcher):
    """A matcher that returns true for any of the forced includes before testing
    against the actual matcher."""

    def __init__(self, matcher, includes):
        super(forceincludematcher, self).__init__(matcher._root, matcher._cwd)
        self._matcher = matcher
        self._includes = set(includes).union([""])

    def __call__(self, value):
        return bool(value in self._includes or self._matcher(value))

    def __repr__(self):
        return "<forceincludematcher matcher=%r includes=%r>" % (
            self._matcher,
            self._includes,
        )

    def visitdir(self, dir):
        if any(True for path in self._includes if path.startswith(dir)):
            return True
        return self._matcher.visitdir(dir)

    def hash(self):
        sha1 = hashlib.sha1()
        sha1.update(_hashmatcher(self._matcher))
        for include in sorted(self._includes):
            sha1.update(include + "\0")
        return sha1.hexdigest()


class unionmatcher(matchmod.unionmatcher):
    def hash(self):
        sha1 = hashlib.sha1()
        for m in self._matchers:
            sha1.update(_hashmatcher(m))
        return sha1.hexdigest()


class negatematcher(matchmod.basematcher):
    def __init__(self, matcher):
        super(negatematcher, self).__init__(matcher._root, matcher._cwd)
        self._matcher = matcher

    def __call__(self, value):
        return not self._matcher(value)

    def __repr__(self):
        return "<negatematcher matcher=%r>" % self._matcher

    def visitdir(self, dir):
        orig = self._matcher.visitdir(dir)
        if orig == "all":
            return False
        elif orig is False:
            return "all"
        else:
            return True

    def hash(self):
        sha1 = hashlib.sha1()
        sha1.update("negate")
        sha1.update(_hashmatcher(self._matcher))
        return sha1.hexdigest()


def _hashmatcher(matcher):
    if util.safehasattr(matcher, "hash"):
        return matcher.hash()

    sha1 = hashlib.sha1()
    sha1.update(repr(matcher))
    return sha1.hexdigest()
