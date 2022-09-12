# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# rcutil.py - utilities about config paths, special config sections etc.
#
#  Copyright Mercurial Contributors
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import errno
import os

from . import config, pycompat, util


if pycompat.iswindows:
    from . import scmwindows as scmplatform
else:
    from . import scmposix as scmplatform

systemrcpath = scmplatform.systemrcpath
userrcpath = scmplatform.userrcpath


def defaultpagerenv():
    """return a dict of default environment variables and their values,
    intended to be set before starting a pager.
    """
    return {"LESS": "FRX", "LV": "-c"}


def editconfig(path, section, name, value):
    """Append a config item to the given config path.

    Try to edit the config in-place without breaking config file syntax for
    simple cases. Fallback to just append the new config.
    """
    path = os.path.realpath(path)
    content = ""
    try:
        content = util.readfileutf8(path)
    except IOError as ex:
        if ex.errno != errno.ENOENT:
            raise
    cfg = config.config()
    cfg.parse("", content, include=lambda *args, **kwargs: None)
    source = cfg.source(section, name)
    edited = False

    # in-place edit if possible
    if source.startswith(":"):
        # line index
        index = int(source[1:]) - 1
        lines = content.splitlines(True)
        # for simple case, we can edit the line in-place
        if (  # config line should still exist
            index < len(lines)
            # the line should start with "NAME ="
            and lines[index].split("=")[0].rstrip() == name
            # the next line should not be indented (a multi-line value)
            and (index + 1 >= len(lines) or not lines[index + 1][:1].isspace())
        ):
            edited = True
            # edit the line
            content = "".join(
                lines[:index]
                + ["%s = %s%s" % (name, value, os.linesep)]
                + lines[index + 1 :]
            )
    if not edited:
        # append as new config
        if content:
            content += os.linesep
        content += "[%s]%s%s = %s%s" % (section, os.linesep, name, value, os.linesep)
    with util.atomictempfile(path) as f:
        f.write(pycompat.encodeutf8(content))
