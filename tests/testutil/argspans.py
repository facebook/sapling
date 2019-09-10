# Copyright 2019 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import inspect
import os


class memoize(dict):
    def __init__(self, func):
        self.func = func

    def __call__(self, *args):
        return self[args]

    def __missing__(self, key):
        result = self[key] = self.func(*key)
        return result


@memoize
def parse(path):
    # Import parso lazily. So importing this module won't error out if parso is
    # not available.
    import parso

    return parso.parse(open(path).read())


def argspans(nested=0):
    """Return argument positions of the function being called.

    The return value is in this form:

        filepath, lineno, indent, spans

    filepath is the absolute file path.

    lineno is the line number as seen by Python (not necessarily the start or
    the end line).

    indent is the number of spaces of the indentation of the function call.

    spans is in this form:

        [((start line, start col), (end line, end col))]

    Line numbers start from 1. Column numbers start from 0.

    `spans, indent` can be `None, 0` if the parsing library (parso) is not
    available, or the callsite location cannot be found.

    If nested is 0, check the function calling `argspans`. If nested is 1, check
    function calling the function calling `argspans`, and so on.
    """
    path, lineno, funcname = sourcelocation(nested + 1)

    def locate(node):
        """Find the node that is the callsite invocation"""
        children = getattr(node, "children", ())
        if len(children) == 2:
            name = children[0]
            if (
                name.type == "name"
                and name.value == funcname
                and node.end_pos[0] >= lineno
                and node.start_pos[0] <= lineno
            ):
                yield node, "args"
                return
        if len(children) == 3 and funcname == "__eq__":
            # Special case: Treat the RHS as "__eq__" args.
            op = children[1]
            if (
                op.type == "operator"
                and op.value == "=="
                and node.end_pos[0] >= lineno
                and node.start_pos[0] <= lineno
            ):
                yield node, "=="
                return
        for c in children:
            for subnode in locate(c):
                yield subnode

    try:
        node, nodetype = next(locate(parse(path)))
    except StopIteration:
        spans = None
        indent = 0
    else:
        if nodetype == "args":
            arglist = node.children[1].children[1]
            assert arglist.type == "arglist"
            # "::2" removes argument separators like ",".
            spans = [(a.start_pos, a.end_pos) for a in arglist.children[::2]]
        elif nodetype == "==":
            rhs = node.children[2]
            spans = [(rhs.start_pos, rhs.end_pos)]
        indent = node.start_pos[1]

    return path, lineno, indent, spans


def sourcelocation(nested=0, _cwd=os.getcwd()):
    """Return (path, lineno, funcname) from Python frames"""
    frame = inspect.currentframe().f_back  # the function calling argspans()
    for _i in range(nested):
        frame = frame.f_back
    funcname = frame.f_code.co_name
    frame = frame.f_back  # the callsite calling "the function" (funcname)
    lineno = frame.f_lineno
    path = os.path.realpath(os.path.join(_cwd, frame.f_code.co_filename))
    return path, lineno, funcname
