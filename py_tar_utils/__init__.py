#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

'''Utilities for dealing with tar files.'''

import os
import tarfile

from typing import List


def from_dirs(dirs: List[str], tar_filename: str):
    '''Create a .tar.gz file from a given list of directories.'''
    with tarfile.open(tar_filename, 'w:gz') as tar:
        for dir in dirs:
            tar.add(dir, arcname=os.path.basename(dir))


def extractall_safe(path: str, dest: str) -> str:
    '''Extract a tar safely. Raise an exception if there are any bad paths.

    Returns the subdirectory the files were extracted to.'''
    tar = tarfile.open(path)
    firstdir = _check_members(path, tar)
    tar.extractall(dest)
    return os.path.join(dest, firstdir)


def _check_members(path: str, tar: tarfile.TarFile) -> str:
    firstdir = None
    for finfo in tar:
        if _badpath(finfo.name):
            raise RuntimeError('{} has bad path: {}'.format(path, finfo.name))
        if firstdir is None:
            firstdir = _firstdir(finfo.name)
        elif firstdir != _firstdir(finfo.name):
            raise RuntimeError(
                '{}: expected path {} to begin with {}'.
                format(path, finfo.name, firstdir)
            )

    if firstdir is None:
        raise RuntimeError('{}: empty tar file'.format(path))

    return firstdir


def _badpath(path: str) -> bool:
    return path.startswith('..') or path.startswith('/')


def _firstdir(rel_path: str) -> str:
    return rel_path.split('/', 1)[0]
