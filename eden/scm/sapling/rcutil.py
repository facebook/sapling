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


import errno
import os

import bindings

from . import util
from .i18n import _


def defaultpagerenv():
    """return a dict of default environment variables and their values,
    intended to be set before starting a pager.
    """
    return {"LESS": "FRX", "LV": "-c"}


def formatconfigvalue(value: str) -> str:
    """Format a config value for writing to a config file.

    Trailing whitespace (including spaces and tabs) is dropped, so a
    whitespace-only value becomes empty. Config parsers already discard this
    whitespace when reading. Embedded newlines are written as indented
    continuation lines, so the value cannot introduce new directives.
    """
    return bindings.configloader.formatvalue(value)


def formatconfigsection(section: str, name: str, value: str) -> str:
    """Format a single-item config section, stripping whitespace around names.

    Values follow the whitespace and continuation rules of formatconfigvalue.
    """
    return bindings.configloader.formatsection(section, name, value)


def formatconfiginclude(path: str) -> str:
    """Format a config include directive, stripping whitespace around its path.

    '#' and ';' are literal path characters in include directives.
    """
    return bindings.configloader.formatinclude(path)


def editconfig(ui, path, section, name, value):
    """Add or remove a config item to the given config path.

    If value is None, delete the config item. Otherwise, strip whitespace
    around section and item names before looking up the item.
    """
    if value is not None:
        section, name = bindings.configloader.normalizekey(section, name)
        formattedvalue = util.tonativeeol(formatconfigvalue(value))

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

    if len(sources) > 1:
        ui.status_err(
            _("warning: duplicate config entries for %s.%s in %s\n")
            % (section, name, path)
        )

    bcontent = content.encode()
    for _value, (_filepath, start, end, _line), _source in reversed(sources):
        if value is None:
            # "start" is the start of value, but we need to remove the
            # "name =" part as well, so back up to beginning of line.
            linestart = bcontent[:start].rfind("\n".encode())
            if linestart == -1:
                linestart = 0

            # On Windows we may or may not have carriage returns. If \r is
            # present, we want to snip it as well.
            if linestart > 0 and bcontent[linestart - 1] == ord("\r"):
                linestart -= 1

            bcontent = bcontent[:linestart] + bcontent[end:]
        else:
            # in-place edit
            # start end are using bytes offset
            bcontent = b"%s%s%s" % (
                bcontent[:start],
                formattedvalue.encode(),
                bcontent[end:],
            )

        break
    else:
        if value is not None:
            # Name doesn't already exist. If section already exists, we want to
            # reuse it, so find the end of the final pre-existing config value as
            # our insert position.
            insertpos = None
            for othername in cfg.names(section):
                for _value, (_filepath, _start, end, _line), _source in cfg.sources(
                    section, othername
                ):
                    if not insertpos or end > insertpos:
                        insertpos = end

            inserttext = "%s%s = %s" % (os.linesep, name, formattedvalue)

            # If the section doesn't already exist we need to append a new section.
            if insertpos is None:
                insertpos = len(bcontent)
                inserttext = util.tonativeeol(formatconfigsection(section, name, value))
                if insertpos > 0:
                    inserttext = "%s%s" % (os.linesep, inserttext)

            bcontent = bcontent[:insertpos] + inserttext.encode() + bcontent[insertpos:]

    os.makedirs(os.path.dirname(path), exist_ok=True)
    with util.atomictempfile(path) as f:
        f.write(bcontent)
