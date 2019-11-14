# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

import os

from edenscm.mercurial import commands, encoding, error, pycompat, util
from edenscm.mercurial.i18n import _

from . import error as ccerror


SERVICE = "commitcloud"
ACCOUNT = "commitcloud"


def getuserconfigpath(ui, overrideconfig):
    """returns the path for per-user configuration

    These paths can be overridden using the given config option.

    Unix:
        returns the home dir, based on 'HOME' environment variable
        if it is set and not equal to the empty string
    Windows:
        returns the value of the 'APPDATA' environment variable
        if it is set and not equal to the empty string
    """
    path = ui.config("commitcloud", overrideconfig)
    if path and not os.path.isdir(path):
        raise ccerror.ConfigurationError(
            ui, _("invalid commitcloud.%s '%s'") % (overrideconfig, path)
        )
    if path:
        return util.expandpath(path)

    if pycompat.iswindows:
        envvar = "APPDATA"
    else:
        envvar = "HOME"
    configpath = encoding.environ.get(envvar)
    if not configpath:
        raise ccerror.ConfigurationError(
            ui, _("$%s environment variable not found") % envvar
        )

    if not os.path.isdir(configpath):
        raise ccerror.ConfigurationError(ui, _("invalid config path '%s'") % configpath)

    return configpath


def getreponame(repo):
    """get the configured reponame for this repo"""
    reponame = repo.ui.config(
        "remotefilelog",
        "reponame",
        os.path.basename(repo.ui.config("paths", "default")),
    )
    if not reponame:
        raise ccerror.ConfigurationError(repo.ui, _("unknown repo"))
    return reponame


def getremotepath(repo, dest):
    # If dest is empty, pass in None to get the default path.
    path = repo.ui.paths.getpath(dest or None, default=("infinitepush", "default"))
    if not path:
        raise error.Abort(
            _("default repository not configured!"),
            hint=_("see 'hg help config.paths'"),
        )
    return path.pushloc or path.loc


def getcommandandoptions(command):
    cmd = commands.table[command][0]
    opts = dict(opt[1:3] for opt in commands.table[command][1])
    return cmd, opts
