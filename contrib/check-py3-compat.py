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
import importlib
import os
import sys
import traceback

def check_compat_py2(f):
    """Check Python 3 compatibility for a file with Python 2"""
    with open(f, 'rb') as fh:
        content = fh.read()
    root = ast.parse(content)

    # Ignore empty files.
    if not root.body:
        return

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

def check_compat_py3(f):
    """Check Python 3 compatibility of a file with Python 3."""
    with open(f, 'rb') as fh:
        content = fh.read()

    try:
        ast.parse(content)
    except SyntaxError as e:
        print('%s: invalid syntax: %s' % (f, e))
        return

    # Try to import the module.
    # For now we only support modules in packages because figuring out module
    # paths for things not in a package can be confusing.
    if (f.startswith(('hgdemandimport/', 'hgext/', 'mercurial/'))
        and not f.endswith('__init__.py')):
        assert f.endswith('.py')
        name = f.replace('/', '.')[:-3]
        try:
            importlib.import_module(name)
        except Exception as e:
            exc_type, exc_value, tb = sys.exc_info()
            # We walk the stack and ignore frames from our custom importer,
            # import mechanisms, and stdlib modules. This kinda/sorta
            # emulates CPython behavior in import.c while also attempting
            # to pin blame on a Mercurial file.
            for frame in reversed(traceback.extract_tb(tb)):
                if frame.name == '_call_with_frames_removed':
                    continue
                if 'importlib' in frame.filename:
                    continue
                if 'mercurial/__init__.py' in frame.filename:
                    continue
                if frame.filename.startswith(sys.prefix):
                    continue
                break

            if frame.filename:
                filename = os.path.basename(frame.filename)
                print('%s: error importing: <%s> %s (error at %s:%d)' % (
                      f, type(e).__name__, e, filename, frame.lineno))
            else:
                print('%s: error importing module: <%s> %s (line %d)' % (
                      f, type(e).__name__, e, frame.lineno))

if __name__ == '__main__':
    if sys.version_info[0] == 2:
        fn = check_compat_py2
    else:
        fn = check_compat_py3

    for f in sys.argv[1:]:
        fn(f)

    sys.exit(0)
