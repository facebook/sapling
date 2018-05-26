# phabdiff.py
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import re

from mercurial import registrar, templatekw
from mercurial.node import hex

from .extlib.phabricator import diffprops


templatekeyword = registrar.templatekeyword()


@templatekeyword("phabdiff")
def showphabdiff(repo, ctx, templ, **args):
    """String. Return the phabricator diff id for a given hg rev."""
    descr = ctx.description()
    revision = diffprops.parserevfromcommitmsg(descr)
    return "D" + revision if revision else ""


@templatekeyword("tasks")
def showtasks(**args):
    """String. Return the tasks associated with given hg rev."""
    tasks = []
    descr = args["ctx"].description()
    match = re.search("(Tasks?|Task ID):(.*)", descr)
    if match:
        tasks = re.findall("\d+", match.group(0))
    return templatekw.showlist("task", tasks, args)


@templatekeyword("singlepublicbase")
def singlepublicbase(repo, ctx, templ, **args):
    """String. Return the public base commit hash."""
    base = repo.revs("last(::%d - not public())", ctx.rev())
    if len(base):
        return hex(repo[base.first()].node())
    return ""
