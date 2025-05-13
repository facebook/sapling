# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# edensparse.py - allow sparse EdenFS checkouts

"""allow sparse EdenFS checkouts

sparse file format
------------------

Structure
.........

Eden sparse files comprise of 3 sections: `[metadata]`, `[include]` and
`[exclude]` sections.

Any line starting with a `;` or `#` character is a comment and is ignored.


Extending existing eden sparse files
....................................


Metadata
........

The `[metadata]` section lets you specify key-value pairs for the profile.
Anything before the first `:` or `=` is the key, everything after is the
value. Values can be extended over multiple lines by indenting additional
lines.

Only the `title`, `description` keys carry meaning for filteredfs.


Include and exclude rules
.........................

Each line in the `[include]` and `[exclude]` sections is treated as a
standard pattern, see :prog:`help patterns`. Exclude rules indicate which
files/patterns should be filtered out from the repository. Include rules
indicate which files should be unfiltered. Everything in the repository is
included (unfiltered) by default.


Example
.......

::

  [metadata]
  title: This is an example filter profile
  description: You can include as much metadata as makes sense for your
    setup, and values can extend over multiple lines.
  lorem ipsum = Keys and values are separated by a : or =

  [include]
  foo/bar/baz
  bar/python_project/**/*.py

  [exclude]
  ; filters follow the "last rule wins" policy. Therefore, the last rule
  ; in the list will take precedence over any earlier rules that conflict
  ; with it.
  foo/bar/baz/*.ignore

"""

from sapling import cmdutil, error, extensions, merge as mergemod, registrar
from sapling.i18n import _

from .sparse import (
    _checknonexistingprofiles,
    _common_config_opts,
    _config,
    _setupcommit,
    _setupupdates,
    _showsubcmdlogic,
    getcommonopts,
    normalizeprofile,
    SparseMixin,
)

cmdtable = {}
command = registrar.command(cmdtable)


def uisetup(ui) -> None:
    if extensions.isenabled(ui, "sparse"):
        return
    _setupupdates(ui)
    _setupcommit(ui)


def reposetup(ui, repo) -> None:
    if "edensparse" not in repo.requirements:
        return

    _wraprepo(ui, repo)


def _wraprepo(ui, repo) -> None:
    class EdenSparseRepo(repo.__class__, SparseMixin):
        def _applysparsetoworkingcopy(
            self, force, origsparsematch, sparsematch, pending
        ):
            self.ui.note(_("applying EdenFS filter to current commit"))
            mergemod.goto(self, self["."], force=force)

    if "dirstate" in repo._filecache:
        repo.dirstate.repo = repo
    repo._sparsecache = {}
    repo.__class__ = EdenSparseRepo


def unimpl():
    raise NotImplementedError("eden sparse support is not implemented yet")


@command(
    "filteredfs",
    [],
    _("SUBCOMMAND ..."),
)
def filteredfs(ui, repo, pat, **opts) -> None:
    """modify the sparseness (AKA filter) of the current eden checkout

    The filteredfs command is used to change the sparseness of a repo. This
    means files that don't meet the filter condition will not be written to
    disk or show up in any working copy operations. It does not affect files
    in history in any way.

    All the work is done in subcommands such as `hg filter enable`. Use the
    `enable` and `disable` subcommands to enable or disable profiles that have
    been committed to the repo. Changes to profiles are not applied until they
    have been committed.

    See :prog:`help filteredfs [subcommand]` to get additional information.
    """
    unimpl()


subcmd = filteredfs.subcommand(
    categories=[
        (
            "Show information about filter profiles",
            ["show"],
        ),
        ("Change which profiles are active", ["enable", "disable"]),
    ]
)


@subcmd(
    "show",
    _common_config_opts + cmdutil.templateopts,
)
def show(ui, repo, **opts) -> None:
    """show the currently enabled filter profile"""
    _showsubcmdlogic(ui, repo, opts)


@subcmd("disable|disableprofile|disablefilter|reset", _common_config_opts)
def disablefiltersubcmd(ui, repo, **opts) -> None:
    """disable the current active filter (i.e. activates the null filter)

    Note: This command does not switch you from a FilteredFS repo to a vanilla
    EdenFS repo. It simply disables the active filter and applies the null
    filter to the working copy.
    """
    commonopts = getcommonopts(opts)
    _config(ui, repo, [], opts, disableprofile=True, **commonopts)


@subcmd(
    "enable|enableprofile|enablefilter|switch|switchprofile|switchfilter",
    _common_config_opts,
    "[FILTER]...",
)
def enablefiltersubcmd(ui, repo, pat, **opts) -> None:
    """enable a filter (disables the previously active filter, if any)"""
    # Filters must not contain colons in their path
    if ":" in pat:
        raise error.Abort(_("filter file paths must not contain ':'"))
    pat = [normalizeprofile(repo, pat)]
    _checknonexistingprofiles(ui, repo, pat)
    commonopts = getcommonopts(opts)
    _config(ui, repo, pat, opts, enableprofile=True, **commonopts)
