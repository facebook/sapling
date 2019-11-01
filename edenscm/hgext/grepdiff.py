# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import re

from edenscm.mercurial import pathutil, registrar, revset, util
from edenscm.mercurial.i18n import _


revsetpredicate = registrar.revsetpredicate()

touchprefix = "touch"
prefixtoprocessors = {
    "add": lambda adds, removes: adds > 0,
    "remove": lambda adds, removes: removes > 0,
    "delta": lambda adds, removes: adds != removes,
    touchprefix: lambda adds, removes: adds > 0 or removes > 0,
    "inc": lambda adds, removes: adds > removes,
    "dec": lambda adds, removes: adds < removes,
}


def getpatternandprocessor(repo, args):
    """Parse prefix and pattern from the provided arguments

    Example argument could be args[0][1] == 'add:hello world'"""
    pattern = args[0][1]
    prefix = touchprefix
    patstart = 0
    if ":" in pattern:
        patstart = pattern.index(":") + 1
        prefix = pattern[: patstart - 1]
    if prefix and prefix not in prefixtoprocessors:
        repo.ui.warning(_("treating %s as a part of pattern") % (prefix + ":"))
        prefix = touchprefix
    else:
        pattern = pattern[patstart:]
    processor = prefixtoprocessors[prefix]
    # currently this regex always has re.M and re.I flags, we might
    # want to make it configurable in future
    pattern = util.re.compile(pattern, re.M | re.I)
    return pattern, processor


@revsetpredicate("grepdiff(pattern, [file], ...)", weight=10)
def grepdiffpredicate(repo, subset, x):
    """grepdiff: a revset for code archeology

    Sample usages are:
      $ hg log --rev "grepdiff('add:command')" mercurial/commands.py
          will only match changesets that add 'command' somewhere in the diff
      $ hg log --rev "grepdiff('remove:command')" mercurial/commands.py
          will match changesets which remove 'command' somewhere in the diff
      $ hg log --rev "grepdiff('delta:command') mercurial/commands.py"
          will mathc changesets where the number of 'command' adds is different
          from the number of 'command' removes in the diff
      $ hg log --rev "grepdiff('touch:command')"
          will only match changesets which either add or remove 'command' at
          least once in the diff
      $ hg log --rev "grepdiff('inc:command')" folder/file1.py folder/file2.py
          will match changesets which increase the number of occurrences
          of 'command' in the specified files
      $ hg log --rev "grepdiff('dec:command')"
          will match changesets which decrease the number of occurrences
          of 'command'
    """
    err = _("wrong set of arguments passed to grepdiff revset")
    args = revset.getargs(x, 1, -1, err)
    files = None
    if len(args) > 1:
        files = set(
            pathutil.canonpath(repo.root, repo.getcwd(), arg[1]) for arg in args[1:]
        )
    pattern, processor = getpatternandprocessor(repo, args)

    def matcher(rev):
        res = processor(*ctxaddsremoves(repo[rev], files, pattern))
        return res

    resset = subset.filter(matcher)
    return resset


def ctxaddsremoves(ctx, files, regexp):
    """Check whether some context matches a given pattern

    'ctx' is a context to check
    'files' is a set of repo-based filenames we're interested in (None
    indicates all files)
    'regexp' is a compiled regular expression against which to match"""
    addcount = 0
    removecount = 0
    filenamelines = []
    for diffitem in ctx.diff():
        # ctx.diff() is a generator that returns a list of strings that are
        # supposed to be printed and some of them are concatenations of
        # multiple '\n'-separated lines. Here's an example of such a list:
        # ["diff --git a/setup.py b/setup.py\n" +\
        #  "--- a/setup.py\n" +\
        #  "+++ b/setup.py\n",
        #  "@@ -1,7 +1,7 @@\n" +\
        #  " from distutils.core import setup, Extension\n" +\
        #  " \n" +\
        #  " setup(\n" +\
        #  "-    name='fbhgextensions',\n" +\
        #  "+    name='fbhgext',\n" +\
        #  "     version='0.1.0',\n" +\
        #  "     author='Durham Goode',\n" +\
        #  "     maintainer='Durham Goode',\n"]
        # Please note that this list in fact contains just two elements, the
        # second string is manually separated into individual lines as they
        # would've been printed.
        # It can be seen that the first element of the list starts with 'diff'
        # and contains the filenames for the upcoming chunks.
        # The second element however has the changes that happened to the
        # file separated by '\n', so we want to parse that, find which ones
        # start with '+' or '-', group them into blocks and match the regex
        # against those blocks.
        if diffitem.startswith("diff"):
            # title line that start diff for some file, does not contain
            # the diff itself. the next iteration of this loop wil hit the
            # actual diff line
            lines = diffitem.split("\n")
            filenamelines = lines[1:3]
            continue

        # a changeblock is a set of consequtive change lines which share the
        # same sign (+/-). we want to join those lines into blocks in order
        # to be able to perform multi-line regex matches
        changeblocks, currentblock, currentsign = [], [], ""
        lines = diffitem.split("\n")
        # an extra iteration is necessary to save the last block
        for line in lines + ["@"]:
            if not line:
                continue
            if line[0] == currentsign:
                # current block continues
                currentblock.append(line[1:])
                continue

            if currentsign:
                # we know that current block is over so we should save it
                changeblocks.append((currentsign, "\n".join(currentblock)))

            if line[0] == "+" or line[0] == "-":
                # new block starts here
                currentsign = line[0]
                currentblock = [line[1:]]
            else:
                # other lines include the ones that start with @@ and
                # contain context line numbers or unchanged context lines
                # from source file.
                currentsign, currentblock = "", []

        beforetablines = (ln.split("\t", 1)[0] for ln in filenamelines)
        filenames = (ln.split("/", 1)[1] for ln in beforetablines if "/" in ln)
        if files and not any(fn for fn in filenames if fn in files):
            # this part of diff does not touch any of the files we're
            # interested in
            continue
        for mod, change in changeblocks:
            match = regexp.search(change)
            if not match:
                continue
            if mod == "+":
                addcount += 1
            else:
                removecount += 1
    return addcount, removecount
