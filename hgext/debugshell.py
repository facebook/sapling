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

import mercurial
from mercurial import demandimport, registrar, thirdparty
from mercurial.i18n import _


cmdtable = {}
command = registrar.command(cmdtable)


def _assignobjects(objects, repo):
    objects.update({"m": mercurial, "mercurial": mercurial})
    if repo:
        objects.update({"repo": repo, "cl": repo.changelog, "mf": repo.manifestlog})

    # Import other handy modules
    for name in ["os", "hgext", "subprocess", "re"]:
        objects[name] = __import__(name)


@command(
    "debugshell|dbsh",
    [("c", "command", "", _("program passed in as string"), _("CMD"))],
    optionalrepo=True,
)
def debugshell(ui, repo, **opts):
    command = opts.get("command")
    if command:
        _assignobjects(locals(), repo)
        exec(command)
        return 0

    bannermsg = (
        "loaded repo:  %s\n"
        "using source: %s" % (repo and repo.root or "(none)", mercurial.__path__[0])
        + "\n\nAvailable variables:\n m:  the mercurial module\n ui: the ui object"
    )
    if repo:
        bannermsg += (
            "\n repo: the repo object\n cl: repo.changelog\n mf: repo.manifestlog"
        )

    # Use bundled IPython. It can be newer and more lightweight than the system
    # package. For a buck build, the IPython dependency is included without
    # using the zip.
    ipypath = os.path.join(os.path.dirname(thirdparty.__file__), "IPython.zip")
    if ipypath not in sys.path and os.path.exists(ipypath):
        sys.path.insert(0, ipypath)

    _assignobjects(locals(), repo)

    # demandimport is incompatible with many IPython dependencies, both at
    # import time and at runtime.
    with demandimport.deactivated():
        import IPython

        IPython.embed(header=bannermsg)
