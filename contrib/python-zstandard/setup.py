#!/usr/bin/env python
# Copyright (c) 2016-present, Gregory Szorc
# All rights reserved.
#
# This software may be modified and distributed under the terms
# of the BSD license. See the LICENSE file for details.

import sys
from setuptools import setup

try:
    import cffi
except ImportError:
    cffi = None

import setup_zstd

SUPPORT_LEGACY = False

if "--legacy" in sys.argv:
    SUPPORT_LEGACY = True
    sys.argv.remove("--legacy")

# Code for obtaining the Extension instance is in its own module to
# facilitate reuse in other projects.
extensions = [setup_zstd.get_c_extension(SUPPORT_LEGACY, 'zstd')]

install_requires = []

if cffi:
    import make_cffi
    extensions.append(make_cffi.ffi.distutils_extension())

    # Need change in 1.8 for ffi.from_buffer() behavior.
    install_requires.append('cffi>=1.8')

version = None

with open('c-ext/python-zstandard.h', 'r') as fh:
    for line in fh:
        if not line.startswith('#define PYTHON_ZSTANDARD_VERSION'):
            continue

        version = line.split()[2][1:-1]
        break

if not version:
    raise Exception('could not resolve package version; '
                    'this should never happen')

setup(
    name='zstandard',
    version=version,
    description='Zstandard bindings for Python',
    long_description=open('README.rst', 'r').read(),
    url='https://github.com/indygreg/python-zstandard',
    author='Gregory Szorc',
    author_email='gregory.szorc@gmail.com',
    license='BSD',
    classifiers=[
        'Development Status :: 4 - Beta',
        'Intended Audience :: Developers',
        'License :: OSI Approved :: BSD License',
        'Programming Language :: C',
        'Programming Language :: Python :: 2.6',
        'Programming Language :: Python :: 2.7',
        'Programming Language :: Python :: 3.3',
        'Programming Language :: Python :: 3.4',
        'Programming Language :: Python :: 3.5',
        'Programming Language :: Python :: 3.6',
    ],
    keywords='zstandard zstd compression',
    ext_modules=extensions,
    test_suite='tests',
    install_requires=install_requires,
)
