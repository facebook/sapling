#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import configparser
import errno
import hashlib
import inspect
import logging
import os
import pathlib
import stat
import sys
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

import eden.config
from eden.test_support.testcase import EdenTestCaseBase
from eden.thrift import legacy

try:
    from eden.thrift import client  # @manual
except ImportError:
    # Thrift-py3 is not supported in the CMake build yet.
    pass

from pathlib import Path

from facebook.eden.ttypes import SourceControlType

from . import edenclient, gitrepo, hgrepo, repobase, skip
from .find_executables import FindExe


if not FindExe.is_buck_build() or os.environ.get("EDENFS_SUFFIX", "") != "":
    _build_flavor = "open_source"
else:
    _build_flavor = "facebook"


class IntegrationTestCase(EdenTestCaseBase):
    pass


@unittest.skipIf(not edenclient.can_run_eden(), "unable to run edenfs")
class EdenTestCase(EdenTestCaseBase):
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

    enable_logview: bool = True

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

        super().setUp()

        # Set an environment variable to prevent telemetry logging
        # during integration tests
        self.setenv("INTEGRATION_TEST", "1")

        self.setup_eden_test()
        self.report_time("test setup done")

    def tearDown(self) -> None:
        self.report_time("clean up started")
        super().tearDown()

    def setup_eden_test(self) -> None:
        # Place scratch configuration somewhere deterministic for the tests
        scratch_config_file = os.path.join(self.tmp_dir, "scratch.toml")
        with open(scratch_config_file, "w") as f:
            f.write(
                'template = "%s"\n'
                % os.path.join(self.tmp_dir, "scratch").replace("\\", "\\\\")
            )
            f.write("overrides = {}\n")
        self.setenv("SCRATCH_CONFIG_PATH", scratch_config_file)

        # Parent directory for any git/hg repositories created during the test
        self.repos_dir = os.path.join(self.tmp_dir, "repos")
        os.mkdir(self.repos_dir)
        # Parent directory for eden mount points
        self.mounts_dir = os.path.join(self.tmp_dir, "mounts")
        os.mkdir(self.mounts_dir)
        self.report_time("temporary directory creation done")

        self.eden = self.init_eden_client()

        # Just to better reflect normal user environments, update $HOME
        # to point to our test home directory for the duration of the test.
        self.setenv("HOME", str(self.eden.home_dir))

        extra_config = self.edenfs_extra_config()
        if extra_config:
            with open(self.eden.system_rc_path, "w") as edenfsrc:
                for key, values in extra_config.items():
                    edenfsrc.write(f"[{key}]\n")
                    for setting in values:
                        edenfsrc.write(f"{setting}\n")

        self.eden.start()
        # Store a lambda in case self.eden is replaced during the test.
        self.addCleanup(lambda: self.eden.cleanup())
        self.report_time("eden daemon started")

        self.mount = os.path.join(self.mounts_dir, "main")

    def init_eden_client(self):
        logging_settings = self.edenfs_logging_settings()
        extra_args = self.edenfs_extra_args()
        if self.enable_fault_injection:
            extra_args.append("--enable_fault_injection")

        if _build_flavor == "facebook" and not self.enable_logview:
            # add option to disable logview
            # we set `EDENFS_SUFFIX` when running our tests with OSS build
            extra_args.append("--eden_logview=false")

        storage_engine = self.select_storage_engine()
        return edenclient.EdenFS(
            base_dir=pathlib.Path(self.tmp_dir),
            logging_settings=logging_settings,
            extra_args=extra_args,
            storage_engine=storage_engine,
        )

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

    def make_temporary_directory(self, prefix: Optional[str] = None) -> str:
        return str(self.temp_mgr.make_temp_dir(prefix=prefix))

    def get_thrift_client_legacy(self) -> legacy.EdenClient:
        """
        Get a thrift client to the edenfs daemon.
        """
        return self.eden.get_thrift_client_legacy()

    def get_thrift_client(self) -> "client.EdenClient":
        """
        Get a streaming thrift client to the edenfs daemon.
        """
        return self.eden.get_thrift_client()

    def get_counters(self) -> typing.Mapping[str, float]:
        with self.get_thrift_client_legacy() as thrift_client:
            thrift_client.flushStatsNow()
            return thrift_client.getCounters()

    def get_backing_dir(self, reponame: str, repo_type: str) -> Path:
        backing_dir_location = Path(self.repos_dir) / reponame
        return (
            backing_dir_location if repo_type == "hg" else backing_dir_location / ".git"
        )

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

    def edenfs_extra_args(self) -> List[str]:
        """
        Get additional arguments to pass to edenfs
        """
        return []

    def edenfs_extra_config(self) -> Optional[Dict[str, List[str]]]:
        """
        Get additional configs to write to the edenfs.rc file before starting
        EdenFS.

        The format is the following:
        {"namespace": ["key1=value1", "key2=value2"}
        """
        configs = {"experimental": ["enable-nfs-server = true"]}
        if self.use_nfs():
            configs["clone"] = ['default-mount-protocol = "NFS"']
        return configs

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

        repo = hgrepo.HgRepository(
            repo_path, system_hgrc=self.system_hgrc, temp_mgr=self.temp_mgr
        )
        repo.init(hgrc=hgrc)

        return repo

    def create_git_repo(self, name: str) -> gitrepo.GitRepository:
        repo_path = os.path.join(self.repos_dir, name)
        os.mkdir(repo_path)
        repo = gitrepo.GitRepository(repo_path, temp_mgr=self.temp_mgr)
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
        with open(fullpath, "wb") as f:
            f.write(contents.encode())
        os.chmod(fullpath, mode)

    def rename(self, from_path: str, to_path: str) -> None:
        """Rename a file/directory at the specified paths relative to the
        clone.
        """
        full_from = self.get_path(from_path)
        full_to = self.get_path(to_path)
        os.rename(full_from, full_to)

    def read_file(self, path: str) -> str:
        """Read the file with the specified path inside the eden repository,
        and return its contents.
        """
        fullpath = self.get_path(path)
        with open(fullpath, "r") as f:
            return f.read()

    def get_expected_file_attributes(
        self, path: str
    ) -> Tuple[bytes, int, SourceControlType]:
        """Get attributes for the file with the specified path inside
        the eden repository. For now, just sha1 and file size.
        """
        fullpath = self.get_path(path)
        file_stat = os.stat(fullpath, follow_symlinks=False)
        file_type = SourceControlType.REGULAR_FILE
        if stat.S_ISDIR(file_stat.st_mode):
            return ((0).to_bytes(20, byteorder="big"), 0, SourceControlType.TREE)
        if stat.S_ISLNK(file_stat.st_mode):
            return ((0).to_bytes(20, byteorder="big"), 0, SourceControlType.SYMLINK)
        if stat.S_IXUSR & file_stat.st_mode:
            file_type = SourceControlType.EXECUTABLE_FILE
        file_size = file_stat.st_size
        ifile = open(fullpath, "rb")
        file_contents = ifile.read()
        sha1_hash = hashlib.sha1(file_contents).digest()
        ifile.close()

        return (sha1_hash, file_size, file_type)

    def mkdir(self, path: str) -> None:
        """Call mkdir for the specified path relative to the clone."""
        full_path = self.get_path(path)
        try:
            os.makedirs(full_path)
        except OSError as ex:
            if ex.errno != errno.EEXIST:
                raise

    def read_dir(self, path: str) -> List[str]:
        fullpath = self.get_path(path)
        return os.listdir(fullpath)

    def make_parent_dir(self, path: str) -> None:
        dirname = os.path.dirname(path)
        if dirname:
            self.mkdir(dirname)

    def rm(self, path: str) -> None:
        """Unlink the file at the specified path relative to the clone."""
        os.unlink(self.get_path(path))

    def rmdir(self, path: str) -> None:
        """Unlink the directory at the specified path relative to the clone."""
        os.rmdir(self.get_path(path))

    def select_storage_engine(self) -> str:
        """
        Prefer to use memory in the integration tests, but allow
        the tests that restart to override this and pick something else.
        """
        return "memory"

    @staticmethod
    def unix_only(fn):
        """
        Decorator that only runs this test on unix platforms.
        """
        if sys.platform == "win32":
            return None
        else:
            return fn

    def use_nfs(self) -> bool:
        """
        Should this test case mount the repo using NFS. This is used by the
        test replication logic to run our integration tests using the default
        mounting method in addition to NFS. This can not be used to disable
        individual tests from using NFS. Individual tests can be disabled
        from running with NFS via skip lists in eden/integration/lib/skip.py.
        """
        return sys.platform == "darwin"


# pyre-ignore[13]: T62487924
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

    enable_logview: bool = False

    # Override is_case_sensitive to True in subclasses to force a case-sensitive
    # clone regardless of the OS, or to False to force a case-insensitive clone.
    # Leave this as None to get the current OS's default case-sensitivity
    # setting.
    #
    # The easiest way to use this is to decorate your test class with
    #
    #   @testcase.eden_repo_test(case_sensitivity_dependent=True)
    #
    # which will generate multiple variants of the test with different
    # is_case_sensitive settings - thus allowing you to easily test multiple
    # case sensitivities on a single platform.
    is_case_sensitive: Optional[bool] = None

    def setup_eden_test(self) -> None:
        super().setup_eden_test()

        self.repo_name = "main"
        self.repo = self.create_repo(self.repo_name)
        self.populate_repo()
        self.report_time("repository setup done")

        self.eden.clone(
            self.repo.path, self.mount, case_sensitive=self.is_case_sensitive
        )
        self.report_time("eden clone done")
        actual_case_sensitive = self.eden.is_case_sensitive(self.mount)
        if self.is_case_sensitive is None:
            self.is_case_sensitive = actual_case_sensitive
        else:
            self.assertEqual(self.is_case_sensitive, actual_case_sensitive)

    def populate_repo(self) -> None:
        raise NotImplementedError(
            "individual test classes must implement populate_repo()"
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
            "test subclasses must implement create_repo().  This is normally"
            " implemented automatically by @eden_repo_test"
        )

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

        def strip_eden_integration_prefix(name: str) -> str:
            prefix = "eden.integration."
            if name.startswith(prefix):
                name = name[len(prefix) :]
            return name

        module = strip_eden_integration_prefix(f"{new_class.__module__}")

        # Allow skipping individual replicated classes, or whole classes.
        class_names = [f"{module}.{name}", f"{module}.{test_class.__name__}"]

        skippedClass = False
        for class_name in class_names:
            if skip.is_class_disabled(class_name):
                skippedClass = True
                break

        if skippedClass:
            # Do not register this class
            continue

        # We also want to be able to skip methods pre replication
        for class_name in class_names:
            for method in dir(new_class):
                if method.startswith("test_"):
                    if skip.is_method_disabled(class_name, method):
                        # A None method will not be listed by unittest causing
                        # the test to never be executed.
                        setattr(new_class, method, None)

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
            raise Exception("we require a python interpreter with stack frame support")
        # pyre-fixme[16]: `Optional` has no attribute `f_locals`.
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


def _replicate_eden_nfs_repo_test(
    test_class: Type[EdenRepoTest],
) -> Iterable[Tuple[str, Type[EdenRepoTest]]]:
    class NFSRepoTest(NFSTestMixin, test_class):
        pass

    class DefaultRepoTest(test_class):
        pass

    variants = [("Default", typing.cast(Type[EdenRepoTest], DefaultRepoTest))]
    # Only run the nfs tests if EdenFS was built with nfs support.
    if eden.config.HAVE_NFS:
        variants.append(("NFS", typing.cast(Type[EdenRepoTest], NFSRepoTest)))

    return variants


# A decorator to duplicate the test to use NFS
#
# Tests that already use eden_repo_test (most of them), do not need to add this
# decorator. However the custom tests that skip this, do need to add this
# decorator.
eden_nfs_repo_test = test_replicator(_replicate_eden_nfs_repo_test)

MixinList = List[Tuple[str, List[Type[Any]]]]


def _replicate_eden_repo_test(
    test_class: Type[EdenRepoTest],
    run_on_nfs: bool = True,
    case_sensitivity_dependent: bool = False,
) -> Iterable[Tuple[str, Type[EdenRepoTest]]]:
    nfs_variants: MixinList = [("", [])]
    if run_on_nfs and eden.config.HAVE_NFS:
        nfs_variants.append(("NFS", [NFSTestMixin]))

    scm_variants: MixinList = [("Hg", [HgRepoTestMixin])]
    # Only run the git tests if EdenFS was built with git support.
    if eden.config.HAVE_GIT:
        scm_variants.append(("Git", [GitRepoTestMixin]))

    case_variants: MixinList = [("", [])]
    if case_sensitivity_dependent:
        case_variants = [
            ("SystemCaseSensitivity", []),
            ("CaseSensitive", [CaseSensitiveTestMixin]),
            ("CaseInsensitive", [CaseInsensitiveTestMixin]),
        ]

    variants = []
    for nfs_label, nfs_mixins in nfs_variants:
        for scm_label, scm_mixins in scm_variants:
            for case_label, case_mixins in case_variants:

                class VariantRepoTest(
                    *nfs_mixins, *scm_mixins, *case_mixins, test_class
                ):
                    pass

                variants.append(
                    (
                        f"{nfs_label}{scm_label}{case_label}",
                        typing.cast(Type[EdenRepoTest], VariantRepoTest),
                    )
                )
    return variants


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
        # pyre-fixme[16]: `HgRepoTestMixin` has no attribute `create_hg_repo`.
        return self.create_hg_repo(name)


class GitRepoTestMixin:
    def create_repo(self, name: str) -> repobase.Repository:
        # pyre-fixme[16]: `GitRepoTestMixin` has no attribute `create_git_repo`.
        return self.create_git_repo(name)


class NFSTestMixin:
    def use_nfs(self) -> bool:
        return True


class CaseSensitiveTestMixin:
    is_case_sensitive = True


class CaseInsensitiveTestMixin:
    is_case_sensitive = False


def _replicate_eden_test(
    test_class: Type[unittest.TestCase],
) -> Iterable[Tuple[str, Type[unittest.TestCase]]]:
    class EdenTest(test_class):
        pass

    return [("Default", typing.cast(Type[unittest.TestCase], EdenTest))]


eden_test = test_replicator(_replicate_eden_test)
