# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Locate and load arcanist configuration for a project

from __future__ import absolute_import

import errno
import json
import os

from edenscm.mercurial import encoding, error, pycompat, registrar


cmdtable = {}
command = registrar.command(cmdtable)


class ArcConfigError(Exception):
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
            raise ArcConfigError(
                "Configuration file %s " "is not a proper JSON file." % filename
            )
        raise


def loadforpath(path):
    # location where `arc install-certificate` writes .arcrc
    if pycompat.iswindows:
        envvar = "APPDATA"
    else:
        envvar = "HOME"
    homedir = encoding.environ.get(envvar)
    if not homedir:
        raise ArcConfigError("$%s environment variable not found" % envvar)

    # Use their own file as a basis
    userconfig = _loadfile(os.path.join(homedir, ".arcrc")) or {}

    # Walk up the path and augment with an .arcconfig if we find it,
    # terminating the search at that point.
    path = os.path.abspath(path)
    while len(path) > 1:
        config = _loadfile(os.path.join(path, ".arcconfig"))
        if config is not None:
            userconfig.update(config)
            # Return the located path too, as we need this for figuring
            # out where we are relative to the fbsource root.
            userconfig["_arcconfig_path"] = path
            return userconfig
        path = os.path.dirname(path)

    raise ArcConfigError("no .arcconfig found")


@command("debugarcconfig")
def debugarcconfig(ui, repo, *args, **opts):
    """ exists purely for testing and diagnostic purposes """
    try:
        config = loadforpath(repo.root)
        ui.write(json.dumps(config, sort_keys=True), "\n")
    except ArcConfigError as ex:
        raise error.Abort(str(ex))
