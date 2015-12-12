#!/usr/bin/env python
#
# check-py3-compat - check Python 3 compatibility of Mercurial files
#
# Copyright 2015 Gregory Szorc <gregory.szorc@gmail.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import, print_function

import ast
import sys

def check_compat(f):
    """Check Python 3 compatibility for a file."""
    with open(f, 'rb') as fh:
        content = fh.read()

    # Ignore empty files.
    if not content.strip():
        return

    root = ast.parse(content)
    futures = set()
    haveprint = False
    for node in ast.walk(root):
        if isinstance(node, ast.ImportFrom):
            if node.module == '__future__':
                futures |= set(n.name for n in node.names)
        elif isinstance(node, ast.Print):
            haveprint = True

    if 'absolute_import' not in futures:
        print('%s not using absolute_import' % f)
    if haveprint and 'print_function' not in futures:
        print('%s requires print_function' % f)

if __name__ == '__main__':
    for f in sys.argv[1:]:
        check_compat(f)

    sys.exit(0)
