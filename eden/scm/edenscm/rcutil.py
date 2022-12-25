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

from . import util


def defaultpagerenv():
    """return a dict of default environment variables and their values,
    intended to be set before starting a pager.
    """
    return {"LESS": "FRX", "LV": "-c"}


def editconfig(path, section, name, value):
    """Add or remove a config item to the given config path.

    If value is None, delete the config item."""
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
    if value and "\n" in value:
        value = value.rstrip("\n").replace("\n", "\n  ")

    bcontent = content.encode()
    for _value, (_filepath, start, end, _line), _source in sources:
        if value is None:
            # "start" is the start of value, but we need to remove the
            # "name =" part as well, so back up to beginning of line.
            linestart = bcontent[:start].rfind(os.linesep.encode())
            if linestart == -1:
                linestart = 0
            bcontent = bcontent[:linestart] + bcontent[end:]
        else:
            # in-place edit
            # start end are using bytes offset
            bcontent = b"%s%s%s" % (bcontent[:start], value.encode(), bcontent[end:])

        break
    else:
        if value is not None:
            # Name doesn't already exist. If section already exists, we want to
            # re-use it, so find the end of the final pre-existing config value as
            # our insert position.
            insertpos = None
            for othername in cfg.names(section):
                for _value, (_filepath, _start, end, _line), _source in cfg.sources(
                    section, othername
                ):
                    if not insertpos or end > insertpos:
                        insertpos = end

            inserttext = "%s%s = %s" % (os.linesep, name, value)

            # If the section doesn't already exist we need to append a new section.
            if insertpos is None:
                insertpos = len(bcontent)
                inserttext = "[%s]%s%s" % (section, inserttext, os.linesep)
                if insertpos > 0:
                    inserttext = "%s%s" % (os.linesep, inserttext)

            bcontent = bcontent[:insertpos] + inserttext.encode() + bcontent[insertpos:]

    with util.atomictempfile(path) as f:
        f.write(bcontent)
