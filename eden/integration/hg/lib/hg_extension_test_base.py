#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

from textwrap import dedent
from typing import List
from ...lib import find_executables, hgrepo, testcase
import configparser
import json
import os


def _find_post_clone():
    post_clone = os.environ.get('EDENFS_POST_CLONE_PATH')
    if not post_clone:
        post_clone = os.path.join(find_executables.BUCK_OUT,
                              'gen/eden/hooks/hg/post-clone.par')
    if not os.access(post_clone, os.X_OK):
        msg = ('unable to find post-clone script for integration testing: {!r}'
                .format(post_clone))
        raise Exception(msg)
    return post_clone


def _eden_ext_dir():
    check_locations = [
        # In dev mode, the python_binary link-tree can be found here:
        'buck-out/gen/eden/hg/eden/eden#link-tree',
        # In other modes, we unpack the python archive here:
        'buck-out/gen/eden/hg/eden/eden/output',
    ]
    for location in check_locations:
        hg_ext_dir = os.path.join(find_executables.REPO_ROOT, location,
                                  'hgext3rd/eden')
        if os.path.isdir(hg_ext_dir):
            return hg_ext_dir

    msg = ('unable to find Hg extension for integration testing: {!r}'
            .format(hg_ext_dir))
    raise Exception(msg)


POST_CLONE = _find_post_clone()
EDEN_EXT_DIR = _eden_ext_dir()


class HgExtensionTestBase(testcase.EdenTestCase):
    '''
    A test case class for integration tests that exercise mercurial commands
    inside an eden client.

    This test case sets up two repositories:
    - self.backing_repo:
      This is the underlying mercurial repository that provides the data for
      the eden mount point.  This has to be populated with an initial commit
      before the eden client is configured, but after initalization most of the
      test interaction will generally be with self.repo instead.

    - self.repo
      This is the hg repository in the eden client.  This is the repository
      where most mercurial commands are actually being tested.
    '''
    def setup_eden_test(self):
        super().setup_eden_test()

        # Create an hgrc to use as the $HGRCPATH.
        hgrc = configparser.ConfigParser()
        hgrc['ui'] = {
            'username': 'Kevin Flynn <lightcyclist@example.com>',
        }
        hgrc['experimental'] = {
            'evolution': 'createmarkers',
            'evolutioncommands': 'prev next split fold obsolete metaedit',
        }
        hgrc['extensions'] = {
            'directaccess': '',
            'fbamend': '',
            'fbhistedit': '',
            'histedit': '',
            'inhibit': '',
            'purge': '',
            'rebase': '',
            'reset': '',
            'strip': '',
            'tweakdefaults': '',
        }
        hgrc['directaccess'] = {
            'loadsafter': 'tweakdefaults',
        }
        self.apply_hg_config_variant(hgrc)

        # Create the backing repository
        self.backing_repo_name = 'backing_repo'
        self.mount = os.path.join(self.mounts_dir, self.backing_repo_name)
        self.backing_repo = self.create_repo(self.backing_repo_name,
                                             hgrepo.HgRepository, hgrc=hgrc)
        self.populate_backing_repo(self.backing_repo)

        self.eden.add_repository(self.backing_repo_name, self.backing_repo.path)
        # Edit the edenrc file to set up post-clone hooks that will correctly
        # populate the .hg directory inside the eden client.
        self.amend_edenrc_before_clone()
        self.eden.clone(self.backing_repo_name, self.mount)

        # Now create the repository object that refers to the eden client
        self.repo = hgrepo.HgRepository(self.mount)

    def populate_backing_repo(self, repo):
        raise NotImplementedError('individual test classes must implement '
                                  'populate_backing_repo()')

    def amend_edenrc_before_clone(self):
        # This is a poor man's version of the generate-hooks-dir script.
        hooks_dir = os.path.join(self.tmp_dir, 'the_hooks')
        os.mkdir(hooks_dir)
        post_clone_hook = os.path.join(hooks_dir, 'post-clone')
        os.symlink(POST_CLONE, post_clone_hook)

        edenrc = os.path.join(os.environ['HOME'], '.edenrc')
        config = configparser.ConfigParser()
        config.read(edenrc)

        config['hooks'] = {}
        config['hooks']['hg.edenextension'] = EDEN_EXT_DIR

        config['repository %s' % self.backing_repo_name]['hooks'] = hooks_dir

        with open(edenrc, 'w') as f:
            config.write(f)

    def hg(self, *args, stdout_charset='utf-8', cwd=None, shell=False,
           hgeditor=None):
        '''Runs `hg.real` with the specified args in the Eden mount.

        If hgeditor is specified, it will be used as the value of the $HGEDITOR
        environment variable when the hg command is run. See
        self.create_editor_that_writes_commit_messages().

        Returns the stdout decoded as a utf8 string. To use a different charset,
        specify the `stdout_charset` as a keyword argument.
        '''
        return self.repo.hg(*args, stdout_charset=stdout_charset, cwd=cwd,
                            shell=shell, hgeditor=hgeditor)

    def create_editor_that_writes_commit_messages(self,
                                                  messages: List[str]) -> str:
        '''
        Creates a program that writes the next message in `messages` to the
        file specified via $1 each time it is invoked.

        Returns the path to the program. This is intended to be used as the
        value for hgeditor in self.hg().
        '''
        tmp_dir = self.tmp_dir

        messages_dir = os.path.join(tmp_dir, 'commit_messages')
        os.makedirs(messages_dir)
        for i, message in enumerate(messages):
            file_name = '{:04d}'.format(i)
            with open(os.path.join(messages_dir, file_name), 'w') as f:
                f.write(message)

        editor = os.path.join(tmp_dir, 'commit_message_editor')

        # Each time this script runs, it takes the "first" message file that is
        # left in messages_dir and moves it to overwrite the path that it was
        # asked to edit. This makes it so that the next time it runs, it will
        # use the "next" message in the queue.
        with open(editor, 'w') as f:
            f.write(
                dedent(
                    f'''\
            #!/bin/bash
            set -e

            for entry in {messages_dir}/*
            do
                mv "$entry" "$1"
                exit 0
            done

            # There was no message to write.
            exit 1
            '''
                )
            )
        os.chmod(editor, 0o755)
        return editor

    def status(self):
        '''Returns the output of `hg status` as a string.'''
        return self.repo.status()

    def assert_status(self, expected, msg=None, check_ignored=True):
        '''Asserts the output of `hg status`. `expected` is a dict where keys
        are paths relative to the repo root and values are the single-character
        string that represents the status: 'M', 'A', 'R', '!', '?', 'I'.

        'C' is not currently supported.
        '''
        args = ['status', '--print0']
        if check_ignored:
            args.append('-mardui')

        output = self.hg(*args)
        actual_status = {}
        for entry in output.split('\0'):
            if not entry:
                continue
            flag = entry[0]
            path = entry[2:]
            actual_status[path] = flag

        self.assertDictEqual(expected, actual_status)

    def assert_status_empty(self, msg=None, check_ignored=True):
        '''Ensures that `hg status` reports no modifications.'''
        self.assert_status({}, msg=msg, check_ignored=check_ignored)

    def assert_copy_map(self, expected):
        stdout = self.eden.run_cmd('debug', 'hg_copy_map_get_all',
                                   cwd=self.mount)
        copy_map = json.loads(stdout)
        self.assertEqual(expected, copy_map)


def _apply_flatmanifest_config(test, config):
    # flatmanifest is the default mercurial behavior
    # no additional config settings are required
    pass


def _apply_treemanifest_config(test, config):
    config['extensions']['fastmanifest'] = ''
    config['extensions']['treemanifest'] = ''
    config['fastmanifest'] = {
        'usetree': 'True',
        'usecache': 'False',
    }
    config['remotefilelog'] = {
        'reponame': 'eden_integration_tests',
        'cachepath': os.path.join(test.tmp_dir, 'hgcache'),
    }


def _apply_treeonly_config(test, config):
    config['extensions']['treemanifest'] = ''
    config['treemanifest'] = {
        'treeonly': 'True',
    }
    config['remotefilelog'] = {
        'reponame': 'eden_integration_tests',
        'cachepath': os.path.join(test.tmp_dir, 'hgcache'),
    }


def _replicate_hg_test(test_class):
    configs = {
        'Flatmanifest': _apply_flatmanifest_config,
        'Treemanifest': _apply_treemanifest_config,
        # TODO: The treemanifest-only tests are currently disabled.
        # The treeonly code in mercurial currently has bugs causing
        # "hg commit" to fail when trying to create the initial root commit in
        # a repository.  We should enable this once the treeonly code is fixed.
        # 'TreeOnly': _apply_treeonly_config,
    }

    for name, config_fn in configs.items():
        class HgTestVariant(test_class, HgExtensionTestBase):
            apply_hg_config_variant = config_fn

        yield name, HgTestVariant


# A decorator function used to define test cases that test eden+mercurial.
#
# This decorator creates multiple TestCase subclasses from a single input
# class.  This allows us to re-run the same test code with several different
# mercurial extension configurations.
#
# The test case subclasses will have different suffixes to identify their
# configuration.  Currently for a given input test class named "MyTest",
# this will create subclasses named:
# - "MyTestFlat": configures hg using the vanilla flat manifest
# - "MyTestTree": configures hg using treemanifest
# - "MyTestTreeOnly": configures hg using treemanifest.treeonly
hg_test = testcase.test_replicator(_replicate_hg_test)
