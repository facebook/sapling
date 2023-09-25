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

from typing import Optional

from sapling import extensions, localrepo, registrar


configtable = {}
configitem = registrar.configitem(configtable)


def _localrepoinit(
    orig, self, baseui, path, create=False, initial_config: Optional[str] = None
):
    orig(self, baseui, path, create, initial_config)

    kwargs = {}

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

    self.ui.log(
        "logginghelper", "", **kwargs  # ui.log requires a format string as args[0].
    )


def uisetup(ui):
    extensions.wrapfunction(localrepo.localrepository, "__init__", _localrepoinit)
