#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from typing import Dict, List, Optional


def tabulate(
    headers: List[str],
    rows: List[Dict[str, str]],
    header_labels: Optional[Dict[str, str]] = None,
) -> str:
    """ Tabulate some data so that it renders reasonably.
    rows - is a list of data that is to be rendered
    headers - is a list of the dictionary keys of the row data to
              be rendered and specifies the order of the fields.
    header_labels - an optional mapping from dictionary key to a
                    more human friendly label for that key.
                    A missing mapping is defaulted to the uppercased
                    version of the key
    Returns a string holding the tabulated result
    """
    col_widths = {}

    def label(name) -> str:
        label = (header_labels or {}).get(name, "")
        if label:
            return label
        return str(name.upper())

    def field(obj, name) -> str:
        return str(obj.get(name, ""))

    for name in headers:
        col_widths[name] = len(label(name))
    for row in rows:
        for name in headers:
            col_widths[name] = max(len(field(row, name)), col_widths[name])

    format_string = ""
    for col_width in col_widths.values():
        if format_string:
            format_string += " "
        format_string += "{:<%d}" % col_width

    output = format_string.format(*[label(name) for name in headers])
    for row in rows:
        output += "\n"
        output += format_string.format(*[field(row, name) for name in headers])
    return output
