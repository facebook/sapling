#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import configparser
import errno
import inspect
import logging
import os
import pathlib
import time
import typing
import unittest
from typing import (
    Any,
    Callable,
    Dict,
    Iterable,
    List,
    Optional,
    Sequence,
    Set,
    Tuple,
    Type,
    Union,
)

from eden.test_support.environment_variable import EnvironmentVariableMixin
from eden.test_support.hypothesis import set_up_hypothesis
from eden.test_support.temporary_directory import TemporaryDirectoryMixin
from eden.thrift import EdenClient
from hypothesis.internal.detection import is_hypothesis_test

from . import edenclient, gitrepo, hgrepo, repobase, util


set_up_hypothesis()


@unittest.skipIf(not edenclient.can_run_eden(), "unable to run edenfs")
class EdenTestCase(
    unittest.TestCase, EnvironmentVariableMixin, TemporaryDirectoryMixin
):
    """
    Base class for eden integration test cases.

    This starts an eden daemon during setUp(), and cleans it up during
    tearDown().
    """

    mount: str
    eden: edenclient.EdenFS
    start: float
    last_event: float

    # Override enable_fault_injection to True in subclasses to enable Eden's fault
    # injection framework when starting edenfs
    enable_fault_injection: bool = False

    # The current typeshed library claims unittest.TestCase.run() returns a TestCase,
    # but it really returns Optional[TestResult].
    # We declare it to return Any here just to make the type checkers happy.
    def run(self, result: Optional[unittest.TestResult] = None) -> Any:
        """ Some slightly awful magic here to arrange for setUp and
            tearDown to be called at the appropriate times when hypothesis
            is enabled for a test case.
            This can be removed once a future version of hypothesis
            ships with support for this baked in. """
        if is_hypothesis_test(getattr(self, self._testMethodName)):
            try:
                old_setUp = self.setUp
                old_tearDown = self.tearDown
                self.setUp = lambda: None  # type: ignore # (mypy issue 2427)
                self.tearDown = lambda: None  # type: ignore # (mypy issue 2427)
                self.setup_example = old_setUp
                self.teardown_example = lambda _: old_tearDown()
                return super(EdenTestCase, self).run(result)
            finally:
                self.setUp = old_setUp  # type: ignore # (mypy issue 2427)
                self.tearDown = old_tearDown  # type: ignore # (mypy issue 2427)
                # pyre-fixme[16]: `EdenTestCase` has no attribute `setup_example`.
                del self.setup_example
                # pyre-fixme[16]: `EdenTestCase` has no attribute `teardown_example`.
                del self.teardown_example
        else:
            return super(EdenTestCase, self).run(result)

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
        self.tmp_dir = self.make_temporary_directory()

        # Parent directory for any git/hg repositories created during the test
        self.repos_dir = os.path.join(self.tmp_dir, "repos")
        os.mkdir(self.repos_dir)
        # Parent directory for eden mount points
        self.mounts_dir = os.path.join(self.tmp_dir, "mounts")
        os.mkdir(self.mounts_dir)
        self.report_time("temporary directory creation done")

        logging_settings = self.edenfs_logging_settings()
        extra_args = self.edenfs_extra_args()
        if self.enable_fault_injection:
            extra_args = extra_args[:] if extra_args is not None else []
            extra_args.append("--enable_fault_injection")

        storage_engine = self.select_storage_engine()
        self.eden = edenclient.EdenFS(
            base_dir=pathlib.Path(self.tmp_dir),
            logging_settings=logging_settings,
            extra_args=extra_args,
            storage_engine=storage_engine,
        )
        # Just to better reflect normal user environments, update $HOME
        # to point to our test home directory for the duration of the test.
        self.set_environment_variable("HOME", str(self.eden.home_dir))
        self.eden.start()
        self.addCleanup(self.eden.cleanup)
        self.report_time("eden daemon started")

        self.mount = os.path.join(self.mounts_dir, "main")

    @property
    def eden_dir(self) -> str:
        return str(self.eden.eden_dir)

    @property
    def home_dir(self) -> str:
        return str(self.eden.home_dir)

    @property
    def etc_eden_dir(self) -> str:
        return str(self.eden.etc_eden_dir)

    @property
    def mount_path(self) -> pathlib.Path:
        return pathlib.Path(self.mount)

    @property
    def mount_path_bytes(self) -> bytes:
        return bytes(self.mount_path)

    def get_thrift_client(self) -> EdenClient:
        """
        Get a thrift client to the edenfs daemon.
        """
        return self.eden.get_thrift_client()

    def get_counters(self) -> typing.Mapping[str, float]:
        with self.get_thrift_client() as thrift_client:
            thrift_client.flushStatsNow()
            return thrift_client.getCounters()

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

    repo: repobase.Repository
    repo_name: str

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

    def get_thrift_client(self) -> EdenClient:
        # get_thrift_client() is also defined in our parent class, but for some reason
        # mypy gets confused when get_thrift_client() is used in our subclasses unless
        # we define it here.  (mypy knows that the method exists but cannot figure out
        # its return type for some reason.)
        return super().get_thrift_client()

    def assert_checkout_root_entries(
        self,
        expected_entries: Set[str],
        path: Union[str, pathlib.Path, None] = None,
        scm_type: Optional[str] = None,
    ) -> None:
        """Verify that the contents of a checkout root directory are what we expect.

        This automatically expects to find a ".hg" directory in the root of hg
        checkouts.
        """
        checkout_root = pathlib.Path(path if path is not None else self.mount)
        real_scm_type = scm_type if scm_type is not None else self.repo.get_type()
        if real_scm_type == "hg":
            expected_entries = expected_entries | {".hg"}
        actual_entries = set(os.listdir(checkout_root))
        self.assertEqual(
            expected_entries, actual_entries, f"incorrect entries in {checkout_root}"
        )


def _replicate_test(
    caller_scope: Dict[str, Any],
    replicate: Callable[..., Iterable[Tuple[str, Type[unittest.TestCase]]]],
    test_class: Type[unittest.TestCase],
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
    replicate: Callable[..., Iterable[Tuple[str, Type[unittest.TestCase]]]]
) -> Callable[..., Any]:
    """
    A helper function for implementing decorators that replicate TestCase
    classes so that the same test function can be run multiple times with
    several different settings.

    See the @eden_repo_test decorator for an example of how this is used.
    """

    def decorator(
        *args: Any, **kwargs: Any
    ) -> Optional[Callable[[Type[unittest.TestCase]], None]]:
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

            def inner_decorator(test_class: Type[unittest.TestCase]) -> None:
                _replicate_test(caller_scope, replicate, test_class, args, kwargs)

            return inner_decorator

    return decorator


def _replicate_eden_repo_test(
    test_class: Type[EdenRepoTest]
) -> Iterable[Tuple[str, Type[EdenRepoTest]]]:
    class HgRepoTest(HgRepoTestMixin, test_class):  # type: ignore
        pass

    class GitRepoTest(GitRepoTestMixin, test_class):  # type: ignore
        pass

    return [
        ("Hg", typing.cast(Type[EdenRepoTest], HgRepoTest)),
        ("Git", typing.cast(Type[EdenRepoTest], GitRepoTest)),
    ]


# A decorator function used to create EdenHgTest and EdenGitTest
# subclasses from a given input test class.
#
# Given an input test class named "MyTest", this will create two separate
# classes named "MyTestHg" and "MyTestGit", which run the tests with
# mercurial and git repositories, respectively.
eden_repo_test = test_replicator(_replicate_eden_repo_test)


class HgRepoTestMixin:
    def create_repo(self, name: str) -> repobase.Repository:
        # HgRepoTestMixin is always used in classes that derive from EdenRepoTest,
        # but it is difficult to make the type checkers aware of that.  We can't
        # add an abstract create_hg_repo() method to this class since the MRO would find
        # it before the real create_hg_repo() name.  We can't change the MRO without
        # breaking resolution of create_repo().
        return self.create_hg_repo(name)  # type: ignore


class GitRepoTestMixin:
    def create_repo(self, name: str) -> repobase.Repository:
        return self.create_git_repo(name)  # type: ignore
