#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
import sys
import unittest
from typing import Dict, List, Union


#
# Test blacklist definitions.
# This is a dictionary of class names. For each class the value can be set to True to
# blacklists all tests in this class, or a list of specific test functions to blacklist.
#
# We are currently blacklisting most existing test cases on Windows, but over time we
# should gradually remove tests from this list as we get them passing on Windows.
#
TEST_BLACKLIST: Dict[str, Union[List[str], bool]] = {}
if sys.platform == "win32":
    # Note that on Windows we also exclude some test source files entirely
    # in CMakeLists.txt, for tests that never make sense to run on Windows.
    TEST_BLACKLIST: Dict[str, Union[List[str], None]] = {
        #
        # Test classes from the main integration test binary
        #
        "basic_test.BasicTestHg": [
            # "edenfsctl remove" does not yet work on Windows,
            # so blacklist related tests
            "test_remove_checkout",
            "test_remove_invalid_paths",
            # The Windows-specific main.cpp file does not yet properly set up
            # fb303 exports and options.
            "test_version",
        ],
        "chown_test.ChownTest": True,
        "clone_test.CloneFakeEdenFSTestAdHoc": True,
        "clone_test.CloneFakeEdenFSTestManaged": True,
        "clone_test.CloneFakeEdenFSTestSystemdEdenCLI": True,
        "clone_test.CloneFakeEdenFSWithSystemdTestSystemdEdenCLI": True,
        "clone_test.CloneTestHg": True,
        "config_test.ConfigTest": True,
        "corrupt_overlay_test.CorruptOverlayTest": True,
        "debug_getpath_test.DebugGetPathTestHg": True,
        "doteden_test.DotEdenTestHg": True,
        "edenclient_test.EdenClientTestHg": True,
        "fsck_test.FsckTest": True,
        "fsck_test.FsckTestNoEdenfs": True,
        "glob_test.GlobTestHg": True,
        "health_test.HealthOfFakeEdenFSTestAdHoc": True,
        "health_test.HealthOfFakeEdenFSTestManaged": True,
        "health_test.HealthOfFakeEdenFSTestSystemdEdenCLI": True,
        "health_test.HealthTest": True,
        "help_test.HelpTest": True,
        "info_test.InfoTestHg": True,
        "linux_cgroup_test.LinuxCgroupTest": True,
        "materialized_query_test.MaterializedQueryTestHg": True,
        "mmap_test.MmapTestHg": True,
        "mount_test.MountTestHg": True,
        "oexcl_test.OpenExclusiveTestHg": True,
        "patch_test.PatchTestHg": True,
        "persistence_test.PersistenceTestHg": True,
        "rage_test.RageTest": True,
        "rc_test.RCTestHg": True,
        "redirect_test.RedirectTestHg": True,
        "remount_test.RemountTestHg": True,
        "rename_test.RenameTestHg": True,
        "restart_test.RestartTestAdHoc": True,
        "restart_test.RestartTestManaged": True,
        "restart_test.RestartTestSystemdEdenCLI": True,
        "restart_test.RestartWithSystemdTestSystemdEdenCLI": True,
        "rocksdb_store_test.RocksDBStoreTest": True,
        "sed_test.SedTestHg": True,
        "service_log_test.ServiceLogFakeEdenFSTestAdHoc": True,
        "service_log_test.ServiceLogFakeEdenFSTestManaged": True,
        "service_log_test.ServiceLogFakeEdenFSTestSystemdEdenCLI": True,
        "service_log_test.ServiceLogRealEdenFSTest": True,
        "setattr_test.SetAttrTestHg": True,
        "stale_test.StaleTest": True,
        "start_test.DirectInvokeTest": True,
        "start_test.StartFakeEdenFSTestAdHoc": True,
        "start_test.StartFakeEdenFSTestManaged": True,
        "start_test.StartFakeEdenFSTestSystemdEdenCLI": True,
        "start_test.StartTest": True,
        "start_test.StartWithRepoTestHg": True,
        "start_test.StartWithSystemdTestSystemdEdenCLI": True,
        "stats_test.CountersTestHg": True,
        "stats_test.FUSEStatsTest": True,
        "stats_test.HgBackingStoreStatsTest": True,
        "stats_test.HgImporterStatsTest": True,
        "stats_test.JournalInfoTestHg": True,
        "stats_test.ObjectStoreStatsTest": True,
        "stop_test.AutoStopTest": True,
        "stop_test.StopTestAdHoc": True,
        "stop_test.StopTestManaged": True,
        "stop_test.StopTestSystemdEdenCLI": True,
        "stop_test.StopWithSystemdTestSystemdEdenCLI": True,
        "takeover_test.TakeoverRocksDBStressTestHg": True,
        "takeover_test.TakeoverTestHg": True,
        "thrift_test.ThriftTestHg": True,
        "unixsocket_test.UnixSocketTestHg": True,
        "unlink_test.UnlinkTestHg": True,
        "userinfo_test.UserInfoTest": True,
        "xattr_test.XattrTestHg": True,
        #
        # Test classes from the hg integration test binary
        #
        "hg.absorb_test.AbsorbTestTreeOnly": True,
        "hg.add_test.AddTestTreeOnly": True,
        "hg.commit_test.CommitTestTreeOnly": True,
        "hg.copy_test.CopyTestTreeOnly": True,
        "hg.debug_clear_local_caches_test.DebugClearLocalCachesTestTreeOnly": True,
        "hg.debug_get_parents.DebugGetParentsTestTreeOnly": True,
        "hg.debug_hg_dirstate_test.DebugHgDirstateTestTreeOnly": True,
        "hg.debug_hg_get_dirstate_tuple_test.DebugHgGetDirstateTupleTestTreeOnly": True,
        "hg.diff_test.DiffTestTreeOnly": True,
        "hg.doctor_test.DoctorTestTreeOnly": True,
        "hg.files_test.FilesTestTreeOnly": True,
        "hg.fold_test.FoldTestTreeOnly": True,
        "hg.graft_test.GraftTestTreeOnly": True,
        "hg.grep_test.GrepTestTreeOnly": True,
        "hg.histedit_test.HisteditTestTreeOnly": True,
        "hg.journal_test.JournalTestTreeOnly": True,
        "hg.merge_test.MergeTestTreeOnly": True,
        "hg.move_test.MoveTestTreeOnly": True,
        "hg.negative_caching_test.NegativeCachingTestTreeOnly": True,
        "hg.non_eden_operation_test.NonEdenOperationTestTreeOnly": True,
        "hg.post_clone_test.SymlinkTestTreeOnly": True,
        "hg.pull_test.PullTestTreeOnly": True,
        "hg.rebase_test.RebaseTestTreeOnly": True,
        "hg.revert_test.RevertTestTreeOnly": True,
        "hg.rm_test.RmTestTreeOnly": True,
        "hg.rollback_test.RollbackTestTreeOnly": True,
        "hg.sparse_test.SparseTestTreeOnly": True,
        "hg.split_test.SplitTestTreeOnly": True,
        "hg.status_deadlock_test.StatusDeadlockTestTreeOnly": True,
        "hg.status_test.StatusTestTreeOnly": [
            # TODO: Opening a file with O_TRUNC inside an EdenFS mount fails on Windows
            "test_partial_truncation_after_open_modifies_file",
            # TODO: These tests do not report the file as modified after truncation
            "test_truncation_after_open_modifies_file",
            "test_truncation_upon_open_modifies_file",
        ],
        "hg.storage_engine_test.HisteditMemoryStorageEngineTestTreeOnly": True,
        "hg.storage_engine_test.HisteditRocksDBStorageEngineTestTreeOnly": True,
        "hg.storage_engine_test.HisteditSQLiteStorageEngineTestTreeOnly": True,
        "hg.symlink_test.SymlinkTestTreeOnly": True,
        "hg.undo_test.UndoTestTreeOnly": True,
        "hg.update_test.UpdateCacheInvalidationTestTreeOnly": True,
        "hg.update_test.UpdateTestTreeOnly": True,
    }
elif sys.platform.startswith("linux") and not os.path.exists("/etc/redhat-release"):
    # The ChownTest.setUp() code tries to look up the "nobody" group, which doesn't
    # exist on Ubuntu.
    TEST_BLACKLIST["chown_test.ChownTest"] = True

    # These tests try to run "hg whereami", which isn't available on Ubuntu.
    # This command is provided by the scm telemetry wrapper rather than by hg
    # itself, and we currently don't install the telemetry wrapper on Ubuntu.
    TEST_BLACKLIST["hg.doctor_test.DoctorTestTreeOnly"] = [
        "test_eden_doctor_fixes_invalid_mismatched_parents",
        "test_eden_doctor_fixes_valid_mismatched_parents",
    ]

    # The systemd_fixture tests have some issues on Ubuntu that I haven't fully
    # investigated yet.
    TEST_BLACKLIST[
        "systemd_fixture_test.TemporarySystemdUserServiceManagerIsolationTest"
    ] = [
        # When run on Ubuntu the path contains some unexpected values like
        # "/usr/games".  I haven't investigated if this is a legitimate issue or
        # not.
        "test_path_environment_variable_is_forced_to_default"
    ]
    TEST_BLACKLIST["systemd_fixture_test.TemporarySystemdUserServiceManagerTest"] = [
        # This test does claim that there are a number of other different units
        # being managed
        "test_no_units_are_active",
        # Running "systemd-analyze --user unit-paths" fails with the error
        # "Unknown operation unit-paths"
        "test_unit_paths_includes_manager_specific_directories",
    ]

    TEST_BLACKLIST["hg.post_clone_test.SymlinkTestTreeOnly"] = [
        # This test fails with mismatched permissions (0775 vs 0755).
        # I haven't investigated too closely but it could be a umask configuration
        # issue.
        "test_post_clone_permissions"
    ]


def skip_test_if_blacklisted(test_case: unittest.TestCase) -> None:
    if _is_blacklisted(test_case):
        raise unittest.SkipTest(f"this test is currently unsupported on this platform")


def _is_blacklisted(test_case: unittest.TestCase) -> bool:
    if not TEST_BLACKLIST:
        return False
    if os.environ.get("EDEN_RUN_BLACKLISTED_TESTS", "") == "1":
        return False

    class_name = f"{type(test_case).__module__}.{type(test_case).__name__}"
    # Strip off the leading "eden.integration." prefix from the module name just
    # to make our blacklisted names shorter and easier to read/maintain.
    strip_prefix = "eden.integration."
    if class_name.startswith(strip_prefix):
        class_name = class_name[len(strip_prefix) :]

    class_blacklist = TEST_BLACKLIST.get(class_name)
    if class_blacklist is None:
        return False
    if isinstance(class_blacklist, bool):
        assert class_blacklist is True
        # All classes in the test are blacklisted
        return True
    else:
        return test_case._testMethodName in class_blacklist
