# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# sampling.py - sample collection extension
#
# Usage:
# - This extension enhances ui.log(category, message, key=value, ...)
# to also append filtered logged events as JSON to a file.
# - The events are separated by NULL characters: '\0'.
# - The file is either specified with the SCM_SAMPLING_FILEPATH environment
# variable or the sampling.filepath configuration.
# - If the file cannot be created or accessed, fails silently
#
# The configuration details can be found in the documentation of ui.log below
import os
import sys
import weakref

from edenscm.mercurial import encoding, localrepo, pycompat, registrar, util


configtable = {}
configitem = registrar.configitem(configtable)

configitem("sampling", "filepath", default="")
configitem("sampling", "debug", default=False)

onehundredmb = 100 * 1024 * 1024


def getrelativecwd(repo):
    """Returns the current directory relative to the working copy root, or
    None if it's not in the working copy.
    """
    cwd = pycompat.getcwdsafe()
    if cwd.startswith(repo.root):
        return os.path.normpath(cwd[len(repo.root) + 1 :])
    else:
        return None


def gettopdir(repo):
    """Returns the first component of the current directory, if it's in the
    working copy.
    """
    reldir = getrelativecwd(repo)
    if reldir:
        components = reldir.split(pycompat.ossep)
        if len(components) > 0 and components[0] != ".":
            return components[0]
    else:
        return None


def telemetry(reporef):
    repo = reporef()
    if repo is None:
        return
    ui = repo.ui
    try:
        try:
            lfsmetrics = repo.svfs.lfsremoteblobstore.getlfsmetrics()
            ui.log("command_metrics", **lfsmetrics)
        except Exception:
            pass

        # Round to the nearest 100MB megabyte to reduce our storage size
        maxrss = int(util.getmaxrss() / onehundredmb) * onehundredmb

        # Log maxrss from within the hg process. The wrapper logs its own
        # value (which is incorrect if chg is used) so the column is
        # prefixed.
        ui.log("command_info", hg_maxrss=maxrss, caller=util.caller())
    except Exception as e:
        ui.log("command_info", sampling_failure=str(e))


def replaceuser(path):
    """Replace path components in ``path`` that match the user's username
    with ``$USER``.
    """
    username = util.username()
    if username is not None:
        sep = os.path.sep
        usercomponent = sep + username + sep
        replacement = sep + "$USER" + sep
        path = path.replace(usercomponent, replacement)
    return path


def reposetup(ui, repo):
    # Don't setup telemetry for sshpeer's
    if not isinstance(repo, localrepo.localrepository):
        return

    repo.ui.atexit(telemetry, weakref.ref(repo))

    # Log other information that we don't want to log in the wrapper, if it's
    # cheap to do so.

    # Log repo root and shared root, with the user's name replaced by "$USER"
    ui.log(
        "command_info",
        reporoot=replaceuser(repo.root),
        reposharedroot=replaceuser(repo.sharedroot),
        python_version=sys.version,
    )

    # Log the current directory bucketed to top-level directories, if enabled.
    # This provides a very rough approximation of what area the users works in.
    # developer config: sampling.logtopdir
    if repo.ui.config("sampling", "logtopdir"):
        topdir = gettopdir(repo)
        if topdir:
            ui.log("command_info", topdir=topdir)

    # Allow environment variables to be directly mapped to metrics columns.
    env = encoding.environ
    tolog = {}
    for conf in ui.configlist("sampling", "env_vars"):
        if conf in env:
            # The default name is a lowercased version of the environment
            # variable name; in the future, an override config could be used to
            # customize it.
            tolog["env_" + conf.lower()] = env[conf]
    ui.log("env_vars", **tolog)
