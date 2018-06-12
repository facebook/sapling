#!/usr/bin/env python3
# Copyright (c) 2004-present, Facebook, Inc.
# All Rights Reserved.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""Given zipped up revlog repos, generate corresponding blob repos."""

import errno
import os
import subprocess
import tempfile
from typing import Optional
from scm import repo

import click

import py_tar_utils


@click.command(help='regenerate blob repo fixtures from revlog repos')
@click.option(
    '--overwrite', is_flag=True, help='overwrite destination if successful'
)
@click.option(
    '--blobimport',
    help='location of blobimport binary',
    type=click.Path(exists=True, dir_okay=False),
)
@click.argument(
    'source', type=click.Path(exists=True, dir_okay=False), nargs=-1
)
def main(source, overwrite, blobimport):
    blobimport = get_blobimport(blobimport)

    if not overwrite:
        for tar in source:
            check_dest(tar)

    for tar in source:
        convert(tar, blobimport)


def get_blobimport(blobimport: Optional[str]) -> str:
    if blobimport is None:
        try:
            from .facebook import pathutils
        except ImportError:
            raise RuntimeError('--blobimport is required')
        return pathutils.get_path('//scm/mononoke:new_blobimport')
    else:
        return blobimport


def check_dest(tar: str):
    basename = os.path.basename(tar)
    dest = basename.split('.', 1)[0]

    if os.path.lexists(dest):
        raise RuntimeError(
            "Destination '{}' already exists (use --overwrite to replace it)".
            format(dest)
        )


def convert(tar: str, blobimport: str):
    basename = os.path.basename(tar)
    dest = basename.split('.', 1)[0]

    abs_dest = os.path.abspath(dest)
    output_dir = os.path.dirname(abs_dest)
    output_name = os.path.basename(abs_dest)

    # We use two separate directories here: tmpdir and tmpdest.
    # - tmpdir is not inside the repository because extracting it would cause a
    #   nested .hg, and fsmonitor isn't very happy with nested .hgs.
    # - tmpdest is in the same directory as the final destination so that it's
    #   on the same filesystem, which guarantees that os.rename works.
    # TODO: support unzipped repos as well?
    with tempfile.TemporaryDirectory(prefix='mononoke-regenerate') as tmpdir:
        revlog_repo = py_tar_utils.extractall_safe(tar, tmpdir)
        # Add mercurial config file to enable treemanifest. This is necessary
        # to back fill treemanifest.
        with open(os.path.join(revlog_repo, '.hg', 'hgrc'), 'w') as hgrc:
            hgrc.write(
                """
[extensions]
treemanifest=
fastmanifest=!

[treemanifest]
treeonly=False
server=True"""
            )

        # Backfill tree manifests
        subprocess.check_call(['hg', '-R', revlog_repo, 'backfilltree'])

        with tempfile.TemporaryDirectory(
            dir=output_dir, prefix=output_name + '.'
        ) as tmpdest:
            new_dir = os.path.join(tmpdest, 'new')
            os.mkdir(new_dir)
            subprocess.check_call(
                [
                    blobimport,
                    '--blobstore',
                    'files',
                    '--repo_id',
                    '1',
                    os.path.join(revlog_repo, '.hg'),
                    new_dir,
                ]
            )

            # clean up
            safe_overwrite_dir(new_dir, abs_dest)
            with open(os.path.join(abs_dest, 'topology'), 'w') as topology_file:
                hgrepo = repo.Repository(revlog_repo, prefer='hg')
                commits = hgrepo.get_commits([':'])
                for commit in commits:
                    parents = ' '.join(
                        (commit.hash for commit in commit.parents())
                    )
                    topology_file.write('%s %s\n' % (commit.hash, parents))


def safe_overwrite_dir(src: str, abs_dest: str):
    output_dir = os.path.dirname(abs_dest)
    output_name = os.path.basename(abs_dest)

    with tempfile.TemporaryDirectory(
        dir=output_dir, prefix=output_name + '.old.'
    ) as to_delete:
        try:
            os.rename(abs_dest, os.path.join(to_delete, 'old'))
        except OSError as ex:
            if ex.errno != errno.ENOENT:
                raise
        os.rename(src, abs_dest)

        # When this block exits, to_delete and therefore the old directory will
        # be deleted.


if __name__ == '__main__':
    main()
