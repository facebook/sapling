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

from sapling import (
    cmdutil,
    error,
    extensions,
    merge as mergemod,
    rcutil,
    registrar,
    util,
)
from sapling.i18n import _

from .sparse import (
    _checknonexistingprofiles,
    _common_config_opts,
    _config,
    _setupcat,
    _setupcommit,
    _setupdiff,
    _setupgrep,
    _setupupdates,
    _showsubcmdlogic,
    getcommonopts,
    normalizeprofile,
    SparseMixin,
)

cmdtable = {}
command = registrar.command(cmdtable)

# Config section and key prefix for filter configs
FILTER_CONFIG_SECTION = "clone"
FILTER_CONFIG_PREFIX = "eden-sparse-filter"
DISABLED_FILTER_CONFIG_PREFIX = "disabled-eden-sparse-filter"


def _get_local_hgrc_path(repo):
    """Get the path to the local repo config file."""
    configfilename = repo.ui.identity.configrepofile()
    return repo.localvfs.join(configfilename)


def _set_filter_config(repo, filter_path, enabled=True):
    """Write a filter config entry to the local repo config file.

    If enabled=True, writes to:
        clone.eden-sparse-filter.<alias> = <filter_path>
    and removes any existing disabled entry.

    If enabled=False, writes to:
        clone.disabled-eden-sparse-filter.<alias> = <filter_path>
    and removes any existing manually enabled entry.
    """
    hgrc_path = _get_local_hgrc_path(repo)
    # Use a sanitized version of the path as the alias/key
    alias = filter_path.replace("/", "_").replace(".", "_")
    enabled_key = "%s.%s" % (FILTER_CONFIG_PREFIX, alias)
    disabled_key = "%s.%s" % (DISABLED_FILTER_CONFIG_PREFIX, alias)

    if enabled:
        rcutil.editconfig(
            repo.ui, hgrc_path, FILTER_CONFIG_SECTION, enabled_key, filter_path
        )
        rcutil.editconfig(repo.ui, hgrc_path, FILTER_CONFIG_SECTION, disabled_key, None)
    else:
        rcutil.editconfig(
            repo.ui, hgrc_path, FILTER_CONFIG_SECTION, disabled_key, filter_path
        )
        rcutil.editconfig(repo.ui, hgrc_path, FILTER_CONFIG_SECTION, enabled_key, None)


def uisetup(ui) -> None:
    if extensions.isenabled(ui, "sparse"):
        return
    _setupupdates(ui)
    _setupcommit(ui)


def reposetup(ui, repo) -> None:
    if "edensparse" not in repo.requirements:
        return

    _wraprepo(ui, repo)


def extsetup(ui) -> None:
    if extensions.isenabled(ui, "sparse"):
        return
    _setupdiff(ui)
    _setupcat(ui)
    _setupgrep(ui)


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


def promptorwarn(ui) -> None:
    """Prompt the user to continue or abort a mutative FilteredFS command.
    Aborts if the user chooses not to continue or if the command is not
    interactive (unless ui.plain() or test environment is detected).
    Logs any bypasses that occur."""
    filter_prompt = (
        ui.config("sparse", "filter-prompt")
        or "Manually modifying filters is not recommended."
    )
    filter_prompt += " Do you still wish to continue? (yn)?$$ &Yes $$ &No"
    default = 0 if ui.plain() or util.istest() else 1
    if ui.promptchoice(_(filter_prompt), default=default) == 0:
        defaulted = default and ui.interactive()
        ui.log(
            "edensparse_prompt",
            prompt_response="defaulted" if defaulted else "bypassed",
        )
        return
    else:
        ui.log("edensparse_prompt", prompt_response="aborted")
        raise error.Abort(
            _("cancelling as requested"),
        )


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


@subcmd("reset", _common_config_opts)
def resetsubcmd(ui, repo, **opts) -> None:
    """disable all filter profiles

    Note: This command does not switch you from a FilteredFS repo to a vanilla
    EdenFS repo. It simply disables all filters and activates the null filter.
    """
    promptorwarn(ui)
    commonopts = getcommonopts(opts)
    _config(ui, repo, [], opts, reset=True, **commonopts)


@subcmd("disable|disableprofile|disablefilter", _common_config_opts)
def disablefiltersubcmd(ui, repo, *pats, **opts) -> None:
    """disable the specified filter.

    Note: This command does not switch you from a FilteredFS repo to a vanilla
    EdenFS repo. It simply disables the specified filter. If all filters are
    disabled, the null filter is activated.
    """
    promptorwarn(ui)
    commonopts = getcommonopts(opts)
    _config(ui, repo, pats, opts, disableprofile=True, **commonopts)


@subcmd(
    "enable|enableprofile|enablefilter",
    _common_config_opts,
    "[FILTER]...",
)
def enablefiltersubcmd(ui, repo, *pats, **opts) -> None:
    """enable a filter"""
    promptorwarn(ui)

    # Filters must not contain colons in their path
    # TODO(cuev): Once V1 profiles are used, we can remove this constraint
    if any(":" in pat for pat in pats):
        raise error.Abort(_("filter file paths must not contain ':'"))
    pats = [normalizeprofile(repo, p) for p in pats]
    _checknonexistingprofiles(ui, repo, pats)
    commonopts = getcommonopts(opts)
    _config(ui, repo, pats, opts, enableprofile=True, **commonopts)


@subcmd("switch|switchprofile|switchfilter", _common_config_opts, "[FILTER]...")
def switchprofilesubcmd(ui, repo, *pats, **opts) -> None:
    """switch to another filter

    Disables all other filters and enables the specified filter(s).
    """
    promptorwarn(ui)
    _checknonexistingprofiles(ui, repo, pats)
    commonopts = getcommonopts(opts)
    _config(ui, repo, pats, opts, reset=True, enableprofile=True, **commonopts)
