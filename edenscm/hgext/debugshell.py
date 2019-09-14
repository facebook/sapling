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

import edenscm
import edenscmnative
from edenscm import hgext, mercurial
from edenscm.mercurial import registrar
from edenscm.mercurial.i18n import _


cmdtable = {}
command = registrar.command(cmdtable)


def _assignobjects(objects, repo):
    objects.update(
        {
            "m": mercurial,
            "e": edenscm,
            "n": edenscmnative,
            "b": edenscmnative.bindings,
            "x": hgext,
            "mercurial": mercurial,
        }
    )
    if repo:
        objects.update({"repo": repo, "cl": repo.changelog, "mf": repo.manifestlog})

    # Import other handy modules
    for name in ["os", "subprocess", "re"]:
        objects[name] = __import__(name)


@command(
    "debugshell|dbsh",
    [("c", "command", "", _("program passed in as string"), _("CMD"))],
    optionalrepo=True,
)
def debugshell(ui, repo, **opts):
    command = opts.get("command")

    _assignobjects(locals(), repo)

    if command:
        exec(command)
        return 0

    if not ui.interactive():
        command = ui.fin.read()
        exec(command)
        return 0

    bannermsg = "loaded repo:  %s\n" "using source: %s" % (
        repo and repo.root or "(none)",
        mercurial.__path__[0],
    ) + (
        "\n\nAvailable variables:\n"
        " e:  edenscm\n"
        " n:  edenscmnative\n"
        " m:  edenscm.mercurial\n"
        " x:  edenscm.hgext\n"
        " b:  edenscmnative.bindings\n"
        " ui: the ui object"
    )
    if repo:
        bannermsg += (
            "\n repo: the repo object\n cl: repo.changelog\n mf: repo.manifestlog"
        )

    import IPython

    IPython.embed(header=bannermsg)
