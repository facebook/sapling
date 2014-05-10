"""Fixer that changes plain strings to bytes strings."""

import re

from lib2to3 import fixer_base
from lib2to3.pgen2 import token
from lib2to3.fixer_util import Name
from lib2to3.pygram import python_symbols as syms

_re = re.compile(r'[rR]?[\'\"]')

# XXX: Implementing a blacklist in 2to3 turned out to be more troublesome than
# blacklisting some modules inside the fixers. So, this is what I came with.

blacklist = ('mercurial/demandimport.py',
             'mercurial/py3kcompat.py', # valid python 3 already
             'mercurial/i18n.py',
            )

def isdocstring(node):
    def isclassorfunction(ancestor):
        symbols = (syms.funcdef, syms.classdef)
        # if the current node is a child of a function definition, a class
        # definition or a file, then it is a docstring
        if ancestor.type == syms.simple_stmt:
            try:
                while True:
                    if ancestor.type in symbols:
                        return True
                    ancestor = ancestor.parent
            except AttributeError:
                return False
        return False

    def ismodule(ancestor):
        # Our child is a docstring if we are a simple statement, and our
        # ancestor is file_input. In other words, our child is a lone string in
        # the source file.
        try:
            if (ancestor.type == syms.simple_stmt and
                ancestor.parent.type == syms.file_input):
                    return True
        except AttributeError:
            return False

    def isdocassignment(ancestor):
        # Assigning to __doc__, definitely a string
        try:
            while True:
                if (ancestor.type == syms.expr_stmt and
                    Name('__doc__') in ancestor.children):
                        return True
                ancestor = ancestor.parent
        except AttributeError:
            return False

    if ismodule(node.parent) or \
       isdocassignment(node.parent) or \
       isclassorfunction(node.parent):
        return True
    return False

def shouldtransform(node):
    specialnames = ['__main__']

    if node.value in specialnames:
        return False

    ggparent = node.parent.parent.parent
    sggparent = str(ggparent)

    if 'getattr' in sggparent or \
       'hasattr' in sggparent or \
       'setattr' in sggparent or \
       'encode' in sggparent or \
       'decode' in sggparent:
        return False

    return True

class FixBytes(fixer_base.BaseFix):

    PATTERN = 'STRING'

    def transform(self, node, results):
        # The filename may be prefixed with a build directory.
        if self.filename.endswith(blacklist):
            return
        if node.type == token.STRING:
            if _re.match(node.value):
                if isdocstring(node):
                    return
                if not shouldtransform(node):
                    return
                new = node.clone()
                new.value = 'b' + new.value
                return new

