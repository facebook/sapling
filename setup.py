#!/usr/bin/env python
# -*- coding: utf-8 -*-
import os
import sys
if not hasattr(sys, 'version_info') or sys.version_info < (2, 4, 0, 'final'):
    raise SystemExit("Mercurial requires python 2.4 or later.")

try:
    from distutils.command.build_py import build_py_2to3 as build_py
except ImportError:
    from distutils.command.build_py import build_py
from distutils.core import setup

setup(
    name = 'hgsubversion',
    version = '0.0.1',
    url = 'http://bitbucket.org/durin42/hgsubversion',
    license = 'GNU GPL',
    author = 'Augie Fackler, others',
    author_email = 'hgsubversion@googlegroups.com',
    description = ('hgsubversion is a Mercurial extension for working with '
                   'Subversion repositories.'),
    long_description = open(os.path.join(os.path.dirname(__file__),
                                         'README')).read(),
    keywords = 'mercurial',
    packages = ('hgsubversion', 'hgsubversion.svnwrap'),
    platforms = 'any',
    classifiers = [
        'License :: OSI Approved :: GNU General Public License (GPL)',
        'Intended Audience :: Developers',
        'Topic :: Software Development :: Version Control',
        'Development Status :: 2 - Pre-Alpha',
        'Programming Language :: Python',
        'Operating System :: OS Independent',
    ],
    cmdclass = {'build_py': build_py},
)
