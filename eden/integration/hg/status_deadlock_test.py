#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import logging
import os
from typing import Callable, Dict, List, Optional

from eden.integration.lib.hgrepo import HgRepository
from .lib.hg_extension_test_base import EdenHgTestCase, hg_test


@hg_test
class StatusDeadlockTest(EdenHgTestCase):
    """
    Test running an "hg status" command that needs to import many directories
    and .gitignore files.

    This attempts to exercise a deadlock issue we had in the past where all of
    the EdenServer thread pool threads would be blocked waiting on operations
    that needed a thread from this pool to complete.  Eden wouldn't be able to
    make forward progress from this state.
    """
    def edenfs_logging_settings(self) -> Dict[str, str]:
        levels = {
            'eden': 'DBG2',
        }
        if logging.getLogger().getEffectiveLevel() <= logging.DEBUG:
            levels['eden.fs.store.hg'] = 'DBG9'
        return levels

    def populate_backing_repo(self, repo: HgRepository) -> None:
        logging.debug('== populate_backing_repo')

        # By default repo.write_file() also calls "hg add" on the path.
        # Unfortunately "hg add" is really slow.  Disable calling it one at a
        # time on these files as we write them.  We'll make a single "hg add"
        # call at the end with all paths in a single command.
        new_files = []

        self.fanouts = [4, 4, 4, 4]

        def populate_dir(path: str) -> None:
            logging.debug('populate %s', path)
            test_path = os.path.join(path, 'test.txt')
            repo.write_file(test_path, f'test\n{path}\n', add=False)
            new_files.append(test_path)

            gitignore_path = os.path.join(path, '.gitignore')
            gitignore_contents = f'*.log\n/{path}/foo.txt\n'
            repo.write_file(gitignore_path, gitignore_contents, add=False)
            new_files.append(gitignore_path)

        self._fanout('src', self.fanouts, populate_dir, populate_dir)

        self._hg_add_many(repo, new_files)

        self.commit1 = repo.commit('Initial commit.')
        logging.debug('== created initial commit')

        new_files = []
        self.expected_status: Dict[str, str] = {}

        def create_new_file(path: str) -> None:
            logging.debug('add new file in %s', path)
            new_path = os.path.join(path, 'new.txt')
            repo.write_file(new_path, 'new\n', add=False)
            new_files.append(new_path)
            self.expected_status[new_path] = '?'

        self._fanout('src', self.fanouts, create_new_file)
        self._hg_add_many(repo, new_files)

        self.commit2 = repo.commit('Initial commit.')
        logging.debug('== created second commit')

    def _hg_add_many(self, repo: HgRepository, paths: List[str]) -> None:
        # Call "hg add" with at most chunk_size files at a time
        chunk_size = 250
        for n in range(0, len(paths), chunk_size):
            logging.debug('= add %d/%d', n, len(paths))
            repo.add_files(paths[n:n + chunk_size])

    def _fanout(self,
                path: str,
                fanouts: List[int],
                leaf_function: Callable[[str], None],
                internal_function: Optional[Callable[[str], None]]=None) -> None:
        '''
        Helper function for recursively building a large branching directory
        tree.

        path is the leading path prefix to put before all directory names.

        fanouts is an array of integers specifying the directory fan-out
        dimensions.  One layer of directories will be created for each element
        in this array.  e.g., [3, 4] would create 3 subdirectories inside the
        top-level directory, and 4 subdirectories in each of those 3
        directories.

        Calls leaf_function on all leaf directories.
        Calls internal_function on all internal (non-leaf) directories.
        '''
        for n in range(fanouts[0]):
            subdir = os.path.join(path, 'dir{:02}'.format(n + 1))
            sub_fanouts = fanouts[1:]
            if sub_fanouts:
                if internal_function is not None:
                    internal_function(subdir)
                self._fanout(subdir, fanouts[1:],
                             leaf_function, internal_function)
            else:
                leaf_function(subdir)

    def test(self) -> None:
        # Reset our working directory parent from commit2 to commit1.
        # This forces us to have an unclean status, but eden won't need
        # to load the source control state yet.  It won't load the affected
        # trees until we run "hg status" below.
        self.hg('reset', '--keep', self.commit1)

        # Now run "hg status".
        # This will cause eden to import all of the trees and .gitignore
        # files as it performs the status operation.
        logging.debug('== checking status')
        self.assert_status(self.expected_status)
