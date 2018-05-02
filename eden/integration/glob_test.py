#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

from facebook.eden.ttypes import EdenError, GlobParams
from .lib import testcase
from typing import List, Optional


@testcase.eden_repo_test
class GlobTest(testcase.EdenRepoTest):
    def populate_repo(self) -> None:
        self.repo.write_file('hello', 'hola\n')
        self.repo.write_file('adir/file', 'foo!\n')
        self.repo.write_file('bdir/file', 'bar!\n')
        self.repo.write_file('bdir/otherfile', 'foo!\n')
        self.repo.symlink('slink', 'hello')
        self.repo.write_file('cdir/subdir/new.txt', 'and improved')
        self.repo.write_file('ddir/notdotfile', '')
        self.repo.write_file('ddir/subdir/notdotfile', '')
        self.repo.write_file('ddir/subdir/.dotfile', '')

        self.repo.write_file('java/com/example/package.html', '')
        self.repo.write_file('java/com/example/Example.java', '')
        self.repo.write_file('java/com/example/foo/Foo.java', '')
        self.repo.write_file('java/com/example/foo/bar/Bar.java', '')
        self.repo.write_file('java/com/example/foo/bar/baz/Baz.java', '')

        self.repo.commit('Commit 1.')

    def setUp(self) -> None:
        super().setUp()
        self.client = self.get_thrift_client()
        self.client.open()
        self.addCleanup(self.client.close)

    def test_exact_path_component_match(self) -> None:
        self.assert_glob(['hello'], ['hello'])
        self.assert_glob(['ddir/subdir/.dotfile'], ['ddir/subdir/.dotfile'])

    def test_wildcard_path_component_match(self) -> None:
        self.assert_glob(['hel*'], ['hello'])
        self.assert_glob(['ad*'], ['adir'])
        self.assert_glob(['a*/file'], ['adir/file'])

    def test_no_accidental_substring_match(self) -> None:
        self.assert_glob(['hell'], [], msg='No accidental substring match')

    def test_match_all_files_in_directory(self) -> None:
        self.assert_glob(['bdir/*'], ['bdir/file', 'bdir/otherfile'])

    def test_match_all_files_in_directory_with_dotfile(self) -> None:
        self.assert_glob(['ddir/subdir/*'], ['ddir/subdir/notdotfile'])

    def test_overlapping_globs(self) -> None:
        self.assert_glob(
            ['adir/*', '**/file'], ['adir/file', 'bdir/file'],
            msg='De-duplicate results from multiple globs'
        )

    def test_recursive_wildcard_prefix(self) -> None:
        self.assert_glob(['**/file'], ['adir/file', 'bdir/file'])

    def test_recursive_wildcard_suffix(self) -> None:
        self.assert_glob(['adir/**'], ['adir/file'])
        self.assert_glob(['adir/**/*'], ['adir/file'])

    def test_recursive_wildcard_suffix_with_dotfile(self) -> None:
        self.assert_glob(
            ['ddir/**'],
            ['ddir/notdotfile', 'ddir/subdir', 'ddir/subdir/notdotfile']
        )
        self.assert_glob(
            ['ddir/**'], [
                'ddir/subdir', 'ddir/subdir/.dotfile', 'ddir/notdotfile',
                'ddir/subdir/notdotfile'
            ],
            include_dotfiles=True
        )

        self.assert_glob(
            ['ddir/**/*'],
            ['ddir/notdotfile', 'ddir/subdir', 'ddir/subdir/notdotfile'],
        )
        self.assert_glob(
            ['ddir/**/*'], [
                'ddir/subdir', 'ddir/subdir/.dotfile', 'ddir/notdotfile',
                'ddir/subdir/notdotfile'
            ],
            include_dotfiles=True
        )

    def test_qualified_recursive_wildcard(self) -> None:
        self.assert_glob(
            ['java/com/**/*.java'], [
                'java/com/example/Example.java',
                'java/com/example/foo/Foo.java',
                'java/com/example/foo/bar/Bar.java',
                'java/com/example/foo/bar/baz/Baz.java',
            ]
        )
        self.assert_glob(
            ['java/com/example/*/*.java'], [
                'java/com/example/foo/Foo.java',
            ]
        )

    def test_malformed_query(self) -> None:
        with self.assertRaises(EdenError) as ctx:
            self.client.glob(self.mount, ['adir['])
        self.assertIn('unterminated bracket sequence', str(ctx.exception))

    def assert_glob(
        self,
        globs: List[str],
        expected_matches: List[str],
        include_dotfiles: bool = False,
        msg: Optional[str] = None
    ) -> None:
        params = GlobParams(self.mount, globs, include_dotfiles)
        self.assertCountEqual(
            expected_matches,
            self.client.globFiles(params).matchingFiles,
            msg=msg
        )

        # Also verify behavior of legacy Thrift API.
        if include_dotfiles:
            self.assertCountEqual(
                expected_matches, self.client.glob(self.mount, globs), msg=msg
            )
