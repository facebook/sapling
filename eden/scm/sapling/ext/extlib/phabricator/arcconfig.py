# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Locate and load arcanist configuration for a project


import errno
import json
import os

from sapling import encoding, error, registrar, util

cmdtable = {}
command = registrar.command(cmdtable)


class ArcConfigError(Exception):
    pass


class ArcConfigLoadError(ArcConfigError):
    pass


class ArcRcMissingCredentials(ArcConfigError):
    pass


def _loadfile(filename):
    try:
        with open(filename, "r") as f:
            return json.loads(f.read())
    except IOError as ex:
        if ex.errno == errno.ENOENT:
            return None
        raise
    except ValueError as ex:
        # if the json file is badly formatted
        if "Expecting property name" in str(ex):
            raise ArcConfigLoadError(
                "Configuration file %s is not a proper JSON file." % filename
            )
        raise


def loadforpath(path):
    # location where `jf authenticate` writes .arcrc
    if util.iswindows:
        envvar = "APPDATA"
    else:
        envvar = "HOME"
    homedir = encoding.environ.get(envvar)
    if not homedir:
        raise ArcConfigLoadError("$%s environment variable not found" % envvar)

    # Use their own file as a basis
    userconfig = _loadfile(os.path.join(homedir, ".arcrc")) or {}

    testtmp = encoding.environ.get("TESTTMP")

    # Walk up the path and augment with an .arcconfig if we find it,
    # terminating the search at that point.
    path = os.path.abspath(path)
    while len(path) > 1:
        if testtmp and os.path.commonprefix([path, testtmp]) != testtmp:
            # Don't allow search for .arcconfig to escape $TESTTMP.
            break

        config = _loadfile(os.path.join(path, ".arcconfig"))
        if config is not None:
            userconfig.update(config)
            # Return the located path too, as we need this for figuring
            # out where we are relative to the fbsource root.
            userconfig["_arcconfig_path"] = path
            return userconfig
        parent = os.path.dirname(path)
        if parent == path or path == homedir:
            # We have reached a root (e.g. "C:\\" on Windows),
            # or reached at homedir (set to TESTTMP in tests),
            # do not look up further. This avoids random files
            # in TMP affects test behavior unexpectedly.
            break
        path = parent

    if not userconfig:
        # We didn't load anything from the .arcrc file, and didn't find a file searching upwards.
        raise ArcConfigLoadError("no .arcconfig found")
    return userconfig


@command("debugarcconfig")
def debugarcconfig(ui, repo, *args, **opts):
    """exists purely for testing and diagnostic purposes"""
    try:
        config = loadforpath(repo.root)
        ui.write(json.dumps(config, sort_keys=True), "\n")
    except ArcConfigError as ex:
        raise error.Abort(str(ex))
