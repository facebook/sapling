#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import atexit
import configparser
import errno
import inspect
import logging
import os
import pathlib
import shutil
import tempfile
import time
import types
import typing
import unittest
from typing import Any, Callable, Dict, Iterable, List, Optional, Sequence, Tuple, Type

import eden.thrift
import hypothesis.strategies as st
from hypothesis import HealthCheck, settings
from hypothesis.configuration import hypothesis_home_dir, set_hypothesis_home_dir
from hypothesis.internal.detection import is_hypothesis_test

from . import edenclient, gitrepo, hgrepo, repobase


def is_sandcastle() -> bool:
    return "SANDCASTLE" in os.environ


default_settings = settings(
    # Turn off the health checks because setUp/tearDown are too slow
    suppress_health_check=[HealthCheck.too_slow],
    # Turn off the example database; we don't have a way to persist this
    # or share this across runs, so we don't derive any benefit from it at
    # this time.
    database=None,
)

# Configure Hypothesis to run faster when iterating locally
settings.register_profile("dev", settings(default_settings, max_examples=5, timeout=0))
# ... and use the defaults (which have more combinations) when running
# on CI, which we want to be more deterministic.
settings.register_profile("ci", settings(default_settings, derandomize=True, timeout=0))

# Use the dev profile by default, but use the ci profile on sandcastle.
settings.load_profile(
    "ci" if is_sandcastle() else os.getenv("HYPOTHESIS_PROFILE", "dev")
)

# Some helpers for Hypothesis decorators
FILENAME_STRATEGY = st.text(
    st.characters(min_codepoint=1, max_codepoint=1000, blacklist_characters="/:\\"),
    min_size=1,
)

# We need to set a global (but non-conflicting) path to store some state
# during hypothesis example runs.  We want to avoid putting this state in
# the repo.
set_hypothesis_home_dir(tempfile.mkdtemp(prefix="eden_hypothesis."))
atexit.register(shutil.rmtree, hypothesis_home_dir())

if not edenclient.can_run_eden():
    # This is avoiding a reporting noise issue in our CI that files
    # tasks about skipped tests.  Let's just skip defining most of them
    # to avoid the noise if we know that they won't work anyway.
    TestParent = typing.cast(Type[unittest.TestCase], object)
else:
    TestParent = unittest.TestCase


def _cleanup_tmp_dir(tmp_dir: str) -> None:
    # "eden clone" makes the original mount point directory read-only.
    # Attempting to delete the files inside it will fail unless we make the directory
    # writable first.
    #
    # If we encounter an EPERM or EACCESS error removing a file try making its parent
    # directory writable and then retry the removal.
    def _remove_readonly(
        func: Callable[[str], Any],
        path: str,
        exc_info: Tuple[Type, BaseException, types.TracebackType],
    ) -> None:
        _ex_type, ex, _traceback = exc_info
        if path == tmp_dir:
            logging.warning(
                f"failed to remove temporary test directory {tmp_dir}: {ex}"
            )
            return
        if not isinstance(ex, PermissionError):
            logging.warning(f"error removing file in temporary directory {path}: {ex}")
            return

        try:
            parent_dir = os.path.dirname(path)
            os.chmod(parent_dir, 0o755)
            # func() is the function that failed.
            # This is usually os.unlink() or os.rmdir().
            func(path)
        except OSError as ex:
            logging.warning(f"error removing file in temporary directory {path}: {ex}")
            return

    shutil.rmtree(tmp_dir, onerror=_remove_readonly)


@unittest.skipIf(not edenclient.can_run_eden(), "unable to run edenfs")
class EdenTestCase(TestParent):
    """
    Base class for eden integration test cases.

    This starts an eden daemon during setUp(), and cleans it up during
    tearDown().
    """

    def run(
        self, report: Optional[unittest.result.TestResult] = None
    ) -> unittest.result.TestResult:
        """ Some slightly awful magic here to arrange for setUp and
            tearDown to be called at the appropriate times when hypothesis
            is enabled for a test case.
            This can be removed once a future version of hypothesis
            ships with support for this baked in. """
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

    def report_time(self, event: str) -> None:
        """
        report_time() is a helper function for logging how long different
        parts of the test took.

        Each time it is called it logs a message containing the time since the
        test started and the time since the last time report_time() was called.
        """
        now = time.time()
        since_last = now - self.last_event
        since_start = now - self.start
        logging.info("=== %s at %.03fs (+%0.3fs)", event, since_start, since_last)
        self.last_event = now

    def setUp(self) -> None:
        self.start = time.time()
        self.last_event = self.start
        self.system_hgrc: Optional[str] = None

        # Add a cleanup event just to log once the other cleanup
        # actions have completed.
        self.addCleanup(self.report_time, "clean up done")

        self.setup_eden_test()
        self.report_time("test setup done")

        self.addCleanup(self.report_time, "clean up started")

    def setup_eden_test(self) -> None:
        def cleanup_tmp_dir() -> None:
            if os.environ.get("EDEN_TEST_NO_CLEANUP"):
                print("Leaving behind eden test directory %r" % self.tmp_dir)
            else:
                _cleanup_tmp_dir(self.tmp_dir)

        self.tmp_dir = tempfile.mkdtemp(prefix="eden_test.")
        self.addCleanup(cleanup_tmp_dir)

        # The home directory, to make sure eden looks at this rather than the
        # real home directory of the user running the tests.
        self.home_dir = os.path.join(self.tmp_dir, "homedir")
        os.mkdir(self.home_dir)
        old_home = os.getenv("HOME")

        def restore_home() -> None:
            if old_home is None:
                del os.environ["HOME"]
            else:
                os.environ["HOME"] = old_home

        os.environ["HOME"] = self.home_dir
        self.addCleanup(restore_home)

        # TODO: Make this configurable via ~/.edenrc.
        # The eden config directory.
        self.eden_dir = os.path.join(self.home_dir, "local/.eden")
        os.makedirs(self.eden_dir)

        self.etc_eden_dir = os.path.join(self.tmp_dir, "etc-eden")
        os.mkdir(self.etc_eden_dir)
        # The directory holding the system configuration files
        self.system_config_dir = os.path.join(self.etc_eden_dir, "config.d")
        os.mkdir(self.system_config_dir)
        # Parent directory for any git/hg repositories created during the test
        self.repos_dir = os.path.join(self.tmp_dir, "repos")
        os.mkdir(self.repos_dir)
        # Parent directory for eden mount points
        self.mounts_dir = os.path.join(self.tmp_dir, "mounts")
        os.mkdir(self.mounts_dir)
        self.report_time("temporary directory creation done")

        logging_settings = self.edenfs_logging_settings()
        extra_args = self.edenfs_extra_args()
        storage_engine = self.select_storage_engine()
        self.eden = edenclient.EdenFS(
            self.eden_dir,
            etc_eden_dir=self.etc_eden_dir,
            home_dir=self.home_dir,
            logging_settings=logging_settings,
            extra_args=extra_args,
            storage_engine=storage_engine,
        )
        self.eden.start()
        self.addCleanup(self.eden.cleanup)
        self.report_time("eden daemon started")

        self.mount = os.path.join(self.mounts_dir, "main")

    @property
    def mount_path(self) -> pathlib.Path:
        return pathlib.Path(self.mount)

    @property
    def mount_path_bytes(self) -> bytes:
        return bytes(self.mount_path)

    def get_thrift_client(self) -> eden.thrift.EdenClient:
        """
        Get a thrift client to the edenfs daemon.
        """
        return self.eden.get_thrift_client()

    def edenfs_logging_settings(self) -> Optional[Dict[str, str]]:
        """
        Get the log settings to pass to edenfs via the --logging argument.

        This should return a dictionary of {category_name: level}
        - module_name is the C++ log category name.  e.g., "eden.fs.store"
          or "eden.fs.inodes.TreeInode"
        - level is the integer vlog level to use for that module.

        You can return None if you do not want any extra verbose logging
        enabled.
        """
        return None

    def edenfs_extra_args(self) -> Optional[List[str]]:
        """
        Get additional arguments to pass to edenfs
        """
        return None

    def create_hg_repo(
        self, name: str, hgrc: Optional[configparser.ConfigParser] = None
    ) -> hgrepo.HgRepository:
        repo_path = os.path.join(self.repos_dir, name)
        os.mkdir(repo_path)

        if self.system_hgrc is None:
            system_hgrc_path = os.path.join(self.repos_dir, "hgrc")
            with open(system_hgrc_path, "w") as f:
                f.write(hgrepo.HgRepository.get_system_hgrc_contents())
            self.system_hgrc = system_hgrc_path

        repo = hgrepo.HgRepository(repo_path, system_hgrc=self.system_hgrc)
        repo.init(hgrc=hgrc)

        return repo

    def create_git_repo(self, name: str) -> gitrepo.GitRepository:
        repo_path = os.path.join(self.repos_dir, name)
        os.mkdir(repo_path)
        repo = gitrepo.GitRepository(repo_path)
        repo.init()

        return repo

    def get_path(self, path: str) -> str:
        """Resolves the path against self.mount."""
        return os.path.join(self.mount, path)

    def touch(self, path: str) -> None:
        """Touch the file at the specified path relative to the clone."""
        fullpath = self.get_path(path)
        with open(fullpath, "a"):
            os.utime(fullpath)

    def write_file(self, path: str, contents: str, mode: int = 0o644) -> None:
        """Create or overwrite a file with the given contents."""
        fullpath = self.get_path(path)
        self.make_parent_dir(fullpath)
        with open(fullpath, "w") as f:
            f.write(contents)
        os.chmod(fullpath, mode)

    def read_file(self, path: str) -> str:
        """Read the file with the specified path inside the eden repository,
        and return its contents.
        """
        fullpath = self.get_path(path)
        with open(fullpath, "r") as f:
            return f.read()

    def mkdir(self, path: str) -> None:
        """Call mkdir for the specified path relative to the clone."""
        full_path = self.get_path(path)
        try:
            os.makedirs(full_path)
        except OSError as ex:
            if ex.errno != errno.EEXIST:
                raise

    def make_parent_dir(self, path: str) -> None:
        dirname = os.path.dirname(path)
        if dirname:
            self.mkdir(dirname)

    def rm(self, path: str) -> None:
        """Unlink the file at the specified path relative to the clone."""
        os.unlink(self.get_path(path))

    def select_storage_engine(self) -> str:
        """
        Prefer to use memory in the integration tests, but allow
        the tests that restart to override this and pick something else.
        """
        return "memory"


class EdenRepoTest(EdenTestCase):
    """
    Base class for EdenHgTest and EdenGitTest.

    This sets up a repository and mounts it before starting each test function.

    You normally should put the @eden_repo_test decorator on your test
    when subclassing from EdenRepoTest.  @eden_repo_test will automatically run
    your tests once per supported repository type.
    """

    def setup_eden_test(self) -> None:
        super().setup_eden_test()

        self.repo_name = "main"
        self.repo = self.create_repo(self.repo_name)
        self.populate_repo()
        self.report_time("repository setup done")

        self.eden.add_repository(self.repo_name, self.repo.path)
        self.eden.clone(self.repo_name, self.mount)
        self.report_time("eden clone done")

    def populate_repo(self) -> None:
        raise NotImplementedError(
            "individual test classes must implement " "populate_repo()"
        )

    def create_repo(self, name: str) -> repobase.Repository:
        """
        Create a new repository.

        Arguments:
        - name
          The repository name.  This determines the repository location inside
          the self.repos_dir directory.  The full repository path can be
          accessed as repo.path on the returned repo object.
        """
        raise NotImplementedError(
            "test subclasses must implement "
            "create_repo().  This is normally "
            "implemented automatically by "
            "@eden_repo_test"
        )


def _replicate_test(
    caller_scope: Dict[str, Any],
    replicate: Callable[..., Iterable[Tuple[str, Type[EdenRepoTest]]]],
    test_class: Type[EdenRepoTest],
    args: Sequence[Any],
    kwargs: Dict[str, Any],
) -> None:
    for suffix, new_class in replicate(test_class, *args, **kwargs):
        # Set the name and module information on our new subclass
        name = test_class.__name__ + suffix
        new_class.__name__ = name
        new_class.__qualname__ = name
        new_class.__module__ = test_class.__module__

        # Add the class to our caller's scope
        caller_scope[name] = new_class


def test_replicator(
    replicate: Callable[..., Iterable[Tuple[str, Type[EdenRepoTest]]]]
) -> Callable[..., Any]:
    """
    A helper function for implementing decorators that replicate TestCase
    classes so that the same test function can be run multiple times with
    several different settings.

    See the @eden_repo_test decorator for an example of how this is used.
    """

    def decorator(
        *args: Any, **kwargs: Any
    ) -> Optional[Callable[[Type[EdenRepoTest]], None]]:
        # We do some rather hacky things here to define new test class types
        # in our caller's scope.  This is needed so that the unittest TestLoader
        # will find the subclasses we define.
        current_frame = inspect.currentframe()
        if current_frame is None:
            raise Exception(
                "we require a python interpreter with " "stack frame support"
            )
        caller_scope = current_frame.f_back.f_locals

        if len(args) == 1 and not kwargs and isinstance(args[0], type):
            # The decorator was invoked directly with the test class,
            # with no arguments or keyword arguments
            _replicate_test(caller_scope, replicate, args[0], args=(), kwargs={})
            return None
        else:

            def inner_decorator(test_class: Type[EdenRepoTest]) -> None:
                _replicate_test(caller_scope, replicate, test_class, args, kwargs)

            return inner_decorator

    return decorator


def _replicate_eden_repo_test(
    test_class: Type[EdenRepoTest]
) -> Iterable[Tuple[str, Type[EdenRepoTest]]]:
    class HgRepoTest(HgRepoTestMixin, test_class):
        pass

    class GitRepoTest(GitRepoTestMixin, test_class):
        pass

    return [("Hg", HgRepoTest), ("Git", GitRepoTest)]


# A decorator function used to create EdenHgTest and EdenGitTest
# subclasses from a given input test class.
#
# Given an input test class named "MyTest", this will create two separate
# classes named "MyTestHg" and "MyTestGit", which run the tests with
# mercurial and git repositories, respectively.
eden_repo_test = test_replicator(_replicate_eden_repo_test)


class HgRepoTestMixin:
    def create_repo(self, name: str) -> repobase.Repository:
        return self.create_hg_repo(name)


class GitRepoTestMixin:
    def create_repo(self, name: str) -> repobase.Repository:
        return self.create_git_repo(name)
