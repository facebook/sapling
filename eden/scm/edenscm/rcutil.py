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

import bindings

from . import pycompat, util


if pycompat.iswindows:
    from . import scmwindows as scmplatform
else:
    from . import scmposix as scmplatform


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
    cfg = bindings.configloader.config()
    cfg.parse(content, source="editconfig")
    sources = cfg.sources(section, name)

    # add necessary indentation to multi-line value
    if "\n" in value:
        value = value.rstrip("\n").replace("\n", "\n  ")

    bcontent = content.encode()
    for _value, (_filepath, start, end, _line), _source in sources:
        # in-place edit
        # start end are using bytes offset
        bcontent = b"%s%s%s" % (bcontent[:start], value.encode(), bcontent[end:])
        break
    else:
        # append as new config
        if bcontent:
            bcontent += os.linesep.encode()
        bcontent += (
            "[%s]%s%s = %s%s" % (section, os.linesep, name, value, os.linesep)
        ).encode()

    with util.atomictempfile(path) as f:
        f.write(bcontent)
