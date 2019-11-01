# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""
myparent.py - Commit template keywords based on your previous commit

If your diff stacks are comprised of related diffs, many commits will share the
same reviewers, tasks and even the title prefix. With this extension, mercurial
can prefill the relevant fields based on your previous commit in the stack.

The extension adds five new keywords:

- *myparentdiff* the diff number of the parent commit
- *myparentreviewers* the reviewers of the parent commit
- *myparentsubscribers* the subscribers of the parent commit
- *myparenttasks* the tasks of the parent commit
- *myparenttitleprefix* the prefix as defined by [] of the parent commit.
                        E.g. '[e2e automation] foo bar' -> '[e2e automation]'

After enabling the extension, change the default commit template:

::

    [committemplate]
    emptymsg={myparenttitleprefix}
      Summary: {myparentdiff}
      Test Plan:
      Reviewers: {myparentreviewers}
      Subscribers: {myparentsubscribers}
      Tasks: {myparenttasks}
      Blame Revision:

In some (all?) repositories at Facebook the commit template is overridden at
the repository level. If that is the case, add the line above to the `.hg/hgrc`
file inside the repository (e.g. ~/www/.hg/hgrc).
"""

import re

from edenscm.mercurial import registrar


templatekeyword = registrar.templatekeyword()


@templatekeyword("myparentdiff")
def showmyparentdiff(repo, ctx, templ, **args):
    """Show the differential revision of the commit's parent, if it has the
       same author as this commit.
    """
    return extract_from_parent(ctx, "Differential Revision:.*/(D\d+)")


@templatekeyword("myparentreviewers")
def showmyparentreviewers(repo, ctx, templ, **args):
    """Show the reviewers of the commit's parent, if it has the
       same author as this commit.
    """
    return extract_from_parent(ctx, "\s*Reviewers: (.*)")


@templatekeyword("myparentsubscribers")
def showmyparentsubscribers(repo, ctx, templ, **args):
    """Show the subscribers of the commit's parent, if it has the
       same author as this commit.
    """
    return extract_from_parent(ctx, "\s*Subscribers: (.*)")


@templatekeyword("myparenttasks")
def showmyparenttasks(repo, ctx, templ, **args):
    """Show the tasks from the commit's parent, if it has the
       same author as this commit.
    """
    return extract_from_parent(ctx, "\s*(?:Tasks|Task ID): (.*)")


@templatekeyword("myparenttitleprefix")
def showmyparenttitleprefix(repo, ctx, templ, **args):
    """Show the title prefix of the commit's parent, if it has the
       same author as this commit.
    """
    if not p1_is_same_user(ctx):
        return ""
    descr = ctx.p1().description()
    title = descr.splitlines()[0]
    prefix_end = title.find("]")
    return title[0 : prefix_end + 1] if prefix_end > 0 else ""


def extract_from_parent(ctx, pattern):
    if not p1_is_same_user(ctx):
        return ""
    descr = ctx.p1().description()
    match = re.search(pattern, descr)
    return match.group(1) if match else ""


def p1_is_same_user(ctx):
    return ctx.user() == ctx.p1().user()
