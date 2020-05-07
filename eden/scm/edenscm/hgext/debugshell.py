# Portions Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Copyright 2010 Mercurial Contributors
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

# debugshell extension
"""a python shell with repo, changelog & manifest objects"""

from __future__ import absolute_import

import code
import os
import sys

import bindings
import edenscm
import edenscmnative
from edenscm import hgext, mercurial
from edenscm.hgext import commitcloud as cc
from edenscm.mercurial import pycompat, registrar
from edenscm.mercurial.i18n import _
from edenscm.mercurial.pycompat import decodeutf8


cmdtable = {}
command = registrar.command(cmdtable)


def _assignobjects(objects, repo):
    objects.update(
        {
            # Shortcuts
            "b": bindings,
            "m": mercurial,
            "x": hgext,
            # Modules
            "bindings": bindings,
            "edenscm": edenscm,
            "edenscmnative": edenscmnative,
            "mercurial": mercurial,
            # Utilities
            "util": mercurial.util,
            "hex": mercurial.node.hex,
            "bin": mercurial.node.bin,
        }
    )
    if repo:
        objects.update(
            {
                "repo": repo,
                "cl": repo.changelog,
                "mf": repo.manifestlog,
                # metalog is not available on hg server-side repos. Consider making it
                # available unconditionally once we get rid of hg servers.
                "ml": getattr(repo.svfs, "metalog", None),
            }
        )

        # Commit cloud service.
        ui = repo.ui
        token = cc.token.TokenLocator(ui).token
        if token is not None:
            objects["serv"] = cc.service.get(ui, token)

    # Import other handy modules
    for name in ["os", "sys", "subprocess", "re"]:
        objects[name] = __import__(name)


@command(
    "debugshell|dbsh|debugsh",
    [("c", "command", "", _("program passed in as string"), _("CMD"))],
    optionalrepo=True,
)
def debugshell(ui, repo, *args, **opts):
    command = opts.get("command")

    _assignobjects(locals(), repo)
    globals().update(locals())
    sys.argv = pycompat.sysargv = args

    if command:
        exec(command)
        return 0
    if args:
        path = args[0]
        with open(path) as f:
            command = f.read()
        globalvars = dict(globals())
        localvars = dict(locals())
        globalvars["__file__"] = path
        exec(command, globalvars, localvars)
        return 0
    elif not ui.interactive():
        command = decodeutf8(ui.fin.read())
        exec(command)
        return 0

    bannermsg = "loaded repo:  %s\n" "using source: %s" % (
        repo and repo.root or "(none)",
        mercurial.__path__[0],
    ) + (
        "\n\nAvailable variables:\n"
        " m:  edenscm.mercurial\n"
        " x:  edenscm.hgext\n"
        " b:  bindings\n"
        " ui: the ui object\n"
    )
    if repo:
        bannermsg += (
            " repo: the repo object\n"
            " serv: commitcloud service\n"
            " cl: repo.changelog\n"
            " mf: repo.manifestlog\n"
            " ml: repo.svfs.metalog\n"
        )

    import IPython

    IPython.embed(header=bannermsg)
