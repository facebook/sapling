# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import re

from edenscm.mercurial import cmdutil, registrar, templatekw
from edenscm.mercurial.node import hex

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
    match = re.search(r"Tasks?([\s-]?ID)?:\s*?[tT\d ,]+", descr)
    if match:
        tasks = re.findall(r"\d+", match.group(0))
    return templatekw.showlist("task", tasks, args)


@templatekeyword("singlepublicbase")
def singlepublicbase(repo, ctx, templ, **args):
    """String. Return the public base commit hash."""
    base = repo.revs("last(::%d - not public())", ctx.rev())
    if len(base):
        return hex(repo[base.first()].node())
    return ""


@templatekeyword("reviewers")
def showreviewers(repo, ctx, templ, **args):
    """String. Return the phabricator diff id for a given hg rev."""
    if ctx.node() is None:
        # working copy - use committemplate.reviewers, which can be found at
        # templ.t.cache.
        props = templ.cache
        reviewersconfig = props.get("reviewers")
        if reviewersconfig:
            return cmdutil.rendertemplate(repo.ui, reviewersconfig, props)
        else:
            return None
    else:
        reviewers = []
        descr = ctx.description()
        match = re.search("Reviewers:(.*)", descr)
        if match:
            reviewers = filter(None, re.split(r"[\s,]", match.group(1)))
        return templatekw.showlist("reviewer", reviewers, args)
