#!/usr/bin/env python
# Copyright (c) 2016-present, Gregory Szorc
# All rights reserved.
#
# This software may be modified and distributed under the terms
# of the BSD license. See the LICENSE file for details.

from setuptools import setup

try:
    import cffi
except ImportError:
    cffi = None

import setup_zstd

# Code for obtaining the Extension instance is in its own module to
# facilitate reuse in other projects.
extensions = [setup_zstd.get_c_extension()]

if cffi:
    import make_cffi
    extensions.append(make_cffi.ffi.distutils_extension())

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
    ],
    keywords='zstandard zstd compression',
    ext_modules=extensions,
    test_suite='tests',
)
