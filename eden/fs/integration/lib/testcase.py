#!/usr/bin/env python3
#
# Copyright (c) 2016, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import atexit
import inspect
import os
import shutil
import tempfile
import unittest
from hypothesis import settings, HealthCheck
import hypothesis.strategies as st
from hypothesis.internal.detection import is_hypothesis_test
from hypothesis.configuration import (
    set_hypothesis_home_dir,
    hypothesis_home_dir)

from . import edenclient
from . import hgrepo
from . import gitrepo


def is_sandcastle():
    return 'SANDCASTLE' in os.environ

default_settings = settings(
    # Turn off the health checks because setUp/tearDown are too slow
    suppress_health_check=[HealthCheck.too_slow],

    # Turn off the example database; we don't have a way to persist this
    # or share this across runs, so we don't derive any benefit from it at
    # this time.
    database=None,
)

# Configure Hypothesis to run faster when iterating locally
settings.register_profile("dev", settings(default_settings, max_examples=5))
# ... and use the defaults (which have more combinations) when running
# on CI, which we want to be more deterministic.
settings.register_profile("ci", settings(default_settings, derandomize=True))

# Use the dev profile by default, but use the ci profile on sandcastle.
settings.load_profile('ci' if is_sandcastle()
                      else os.getenv('HYPOTHESIS_PROFILE', 'dev'))

# Some helpers for Hypothesis decorators
FILENAME_STRATEGY = st.text(
    st.characters(min_codepoint=1,
                  max_codepoint=1000,
                  blacklist_characters="/:\\",
                  ),
    min_size=1)

# We need to set a global (but non-conflicting) path to store some state
# during hypothesis example runs.  We want to avoid putting this state in
# the repo.
set_hypothesis_home_dir(tempfile.mkdtemp(prefix='eden_hypothesis.'))
atexit.register(shutil.rmtree, hypothesis_home_dir())

if is_sandcastle() and not edenclient.can_run_eden():
    # This is avoiding a reporting noise issue in our CI that files
    # tasks about skipped tests.  Let's just skip defining most of them
    # to avoid the noise if we know that they won't work anyway.
    TestParent = object
else:
    TestParent = unittest.TestCase


@unittest.skipIf(not edenclient.can_run_eden(), "unable to run edenfs")
class EdenTestCase(TestParent):
    '''
    Base class for eden integration test cases.

    This starts an eden daemon during setUp(), and cleans it up during
    tearDown().
    '''

    def run(self, report=None):
        ''' Some slightly awful magic here to arrange for setUp and
            tearDown to be called at the appropriate times when hypothesis
            is enabled for a test case.
            This can be removed once a future version of hypothesis
            ships with support for this baked in. '''
        if is_hypothesis_test(getattr(self, self._testMethodName)):
            try:
                old_setUp = self.setUp
                old_tearDown = self.tearDown
                self.setUp = lambda: None
                self.tearDown = lambda: None
                self.setup_example = old_setUp
                self.teardown_example = lambda _: old_tearDown()
                return super(EdenTestCase, self).run(report)
            finally:
                self.setUp = old_setUp
                self.tearDown = old_tearDown
                del self.setup_example
                del self.teardown_example
        else:
            return super(EdenTestCase, self).run(report)

    def setUp(self):
        self.tmp_dir = None
        self.eden = None
        self.old_home = None

        # Call setup_eden_test() to do most of the setup work, and call
        # tearDown() on any error.  tearDown() won't be called by default if
        # setUp() throws.
        try:
            self.setup_eden_test()
        except Exception as ex:
            self.tearDown()
            raise

    def setup_eden_test(self):
        self.tmp_dir = tempfile.mkdtemp(prefix='eden_test.')

        # The eden config directory
        self.eden_dir = os.path.join(self.tmp_dir, 'eden')
        # The home directory, to make sure eden looks at this rather than the
        # real home directory of the user running the tests.
        self.home_dir = os.path.join(self.tmp_dir, 'homedir')
        os.mkdir(self.home_dir)
        self.old_home = os.getenv('HOME')
        os.environ['HOME'] = self.home_dir
        # The directory holding the system configuration files
        self.system_config_dir = os.path.join(self.tmp_dir, 'config.d')
        os.mkdir(self.system_config_dir)
        # Parent directory for any git/hg repositories created during the test
        self.repos_dir = os.path.join(self.tmp_dir, 'repos')
        os.mkdir(self.repos_dir)
        # Parent directory for eden mount points
        self.mounts_dir = os.path.join(self.tmp_dir, 'mounts')
        os.mkdir(self.mounts_dir)

        self.eden = edenclient.EdenFS(self.eden_dir,
                                      system_config_dir=self.system_config_dir,
                                      home_dir=self.home_dir)
        self.eden.start()

    def tearDown(self):
        error = None
        try:
            if self.eden is not None:
                self.eden.cleanup()
        except Exception as ex:
            error = ex

        if self.old_home is not None:
            os.environ['HOME'] = self.old_home
            self.old_home = None

        if self.tmp_dir is not None:
            shutil.rmtree(self.tmp_dir, ignore_errors=True)
            self.tmp_dir = None

        # Re-raise any error that occurred, after we finish
        # trying to clean up our directories.
        if error is not None:
            raise error

    def get_thrift_client(self):
        '''
        Get a thrift client to the edenfs daemon.
        '''
        return self.eden.get_thrift_client()

    def create_repo(self, name, repo_class):
        '''
        Create a new repository.

        Arguments:
        - name
          The repository name.  This determines the repository location inside
          the self.repos_dir directory.  The full repository path can be
          accessed as repo.path on the returned repo object.
        - repo_class
          The repository class object, such as hgrepo.HgRepository or
          gitrepo.GitRepository.
        '''
        repo_path = os.path.join(self.repos_dir, name)
        os.mkdir(repo_path)
        repo = repo_class(repo_path)
        repo.init()

        return repo


class EdenRepoTestBase(EdenTestCase):
    '''
    Base class for EdenHgTest and EdenGitTest.

    This sets up a repository and mounts it before starting each test function.
    '''
    def setup_eden_test(self):
        super().setup_eden_test()

        self.repo_name = 'main'
        self.mount = os.path.join(self.mounts_dir, self.repo_name)

        self.repo = self.create_repo(self.repo_name, self.get_repo_class())
        self.populate_repo()

        self.eden.add_repository(self.repo_name, self.repo.path)
        self.eden.clone(self.repo_name, self.mount)

    def populate_repo(self):
        raise NotImplementedError('individual test classes must implement '
                                  'populate_repo()')


class EdenHgTest(EdenRepoTestBase):
    '''
    Subclass of EdenTestCase which uses a single mercurial repository and
    eden mount.

    The repository is available as self.repo, and the client mount path is
    available as self.mount
    '''
    def get_repo_class(self):
        return hgrepo.HgRepository


class EdenGitTest(EdenRepoTestBase):
    '''
    Subclass of EdenTestCase which uses a single mercurial repository and
    eden mount.

    The repository is available as self.repo, and the client mount path is
    available as self.mount
    '''
    def get_repo_class(self):
        return gitrepo.GitRepository


def eden_repo_test(test_class):
    '''
    A decorator function used to create EdenHgTest and EdenGitTest
    subclasses from a given input test class.

    Given an input test class named "MyTest", this will create two separate
    classes named "MyTestHg" and "MyTestGit", which run the tests with
    mercurial and git repositories, respectively.
    '''
    repo_types = [
        (EdenHgTest, 'Hg'),
        (EdenGitTest, 'Git'),
    ]

    # We do some rather hacky things here to define new test class types
    # in our caller's scope.  This is needed so that the unittest TestLoader
    # will find the subclasses we define.
    caller_scope = inspect.currentframe().f_back.f_locals

    for (parent_class, suffix) in repo_types:
        subclass_name = test_class.__name__ + suffix

        # Define a new class that derives from the input class
        # as well as the repo-specific parent class type
        class RepoSpecificTest(test_class, parent_class):
            pass

        # Set the name and module information on our new subclass
        RepoSpecificTest.__name__ = subclass_name
        RepoSpecificTest.__qualname__ = subclass_name
        RepoSpecificTest.__module__ = test_class.__module__

        caller_scope[subclass_name] = RepoSpecificTest

    return None
