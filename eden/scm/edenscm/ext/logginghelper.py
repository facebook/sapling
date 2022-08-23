# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# reporootlog.py - log the repo root

"""this extension logs different pieces of information that will be used
by SCM wrappers

::

    [loggedconfigs]
    # list of config options to log
    name1 = section1.option1
    name2 = section2.option2

"""

import os

from edenscm.mercurial import extensions, localrepo, registrar


configtable = {}
configitem = registrar.configitem(configtable)


def _localrepoinit(orig, self, baseui, path=None, create=False):
    orig(self, baseui, path, create)
    reponame = self.ui.config("paths", "default")
    if reponame:
        reponame = os.path.basename(reponame).split("?")[0]
    if path and not reponame:
        reponame = os.path.basename(path)
    kwargs = {"repo": reponame}

    # The configs being read here are user defined, so we need to suppress
    # warnings telling us to register them.
    with self.ui.configoverride({("devel", "all-warnings"): False}):
        for targetname, option in self.ui.configitems("loggedconfigs"):
            split = option.split(".")
            if len(split) != 2:
                continue
            section, name = split
            value = self.ui.config(section, name)
            if value is not None:
                kwargs[targetname] = value

    if "treestate" in self.requirements:
        dirstateformat = "treestate"
    elif "treedirstate" in self.requirements:
        dirstateformat = "treedirstate"
    else:
        dirstateformat = "flatdirstate"

    kwargs["dirstate_format"] = dirstateformat

    self.ui.log(
        "logginghelper", "", **kwargs  # ui.log requires a format string as args[0].
    )


def uisetup(ui):
    extensions.wrapfunction(localrepo.localrepository, "__init__", _localrepoinit)
