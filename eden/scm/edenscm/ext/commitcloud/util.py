# Copyright (c) Meta Platforms, Inc. and affiliates.
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
FAKE_TOKEN = "fake_token"


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
    reponame = repo.ui.config("remotefilelog", "reponame")
    if not reponame:
        raise ccerror.ConfigurationError(repo.ui, _("unknown repo"))
    return reponame


def getnullableremotepath(ui):
    """Select an appopriate remote repository to connect to for commit cloud operations."""
    if "default" not in ui.paths:
        return None
    path = ui.paths.getpath("default")
    return path.pushloc or path.loc


def getremotepath(ui):
    path = getnullableremotepath(ui)
    if not path:
        raise error.Abort(
            _("'default' repository isn't configured!"),
            hint=_("see 'hg help config.paths'"),
        )
    return path


def getcommandandoptions(command):
    cmd = commands.table[command][0]
    opts = dict(opt[1:3] for opt in commands.table[command][1])
    return cmd, opts
