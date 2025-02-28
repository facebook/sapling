#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

import abc
import configparser
import itertools
import json
import logging
import os
import re
import sys
import textwrap
import time
import typing
from pathlib import Path
from textwrap import dedent
from typing import Any, Dict, Iterable, List, Optional, Set, Tuple, Type, Union

import eden.config
from eden.integration.lib import hgrepo, testcase

from eden.integration.lib.find_executables import FindExe


def get_default_hgrc() -> configparser.ConfigParser:
    """
    Get the default hgrc settings to use in the backing store repository.

    This returns the base settings, which can then be further adjusted by test
    cases and test case variants.
    """
    hgrc = configparser.ConfigParser()
    hgrc["ui"] = {
        "origbackuppath": ".hg/origbackups",
        "username": "Kevin Flynn <lightcyclist@example.com>",
    }
    hgrc["experimental"] = {
        "evolution": "createmarkers",
        "evolutioncommands": "prev next split fold obsolete metaedit",
    }
    hgrc["extensions"] = {
        "absorb": "",
        "amend": "",
        "directaccess": "",
        "fbhistedit": "",
        "histedit": "",
        "journal": "",
        "purge": "",
        "rebase": "",
        "reset": "",
        "sparse": "",
        "strip": "",
        "tweakdefaults": "",
        "undo": "",
    }
    hgrc["directaccess"] = {"loadsafter": "tweakdefaults"}
    hgrc["diff"] = {"git": "True"}
    return hgrc


class EdenHgTestCase(testcase.EdenTestCase, metaclass=abc.ABCMeta):
    """
    A test case class for integration tests that exercise mercurial commands
    inside an eden client.

    This test case sets up two repositories:
    - self.backing_repo:
      This is the underlying mercurial repository that provides the data for
      the eden mount point.  This has to be populated with an initial commit
      before the eden client is configured, but after initialization most of the
      test interaction will generally be with self.repo instead.

    - self.repo
      This is the hg repository in the eden client.  This is the repository
      where most mercurial commands are actually being tested.
    """

    repo: hgrepo.HgRepository
    backing_repo: hgrepo.HgRepository
    enable_windows_symlinks: bool = False
    inode_catalog_type: Optional[str] = None
    backing_store_type: Optional[str] = None
    adtl_repos: List[Tuple[hgrepo.HgRepository, Optional[hgrepo.HgRepository]]] = []
    enable_status_cache: bool = False

    def setup_eden_test(self) -> None:
        super().setup_eden_test()

        # Create the backing repository
        self.backing_repo = self.create_backing_repo()

        # Edit the edenrc file to set up post-clone hooks that will correctly
        # populate the .hg directory inside the eden client.
        self.eden.clone(
            self.backing_repo.path,
            self.mount,
            allow_empty=True,
            enable_windows_symlinks=self.enable_windows_symlinks,
            backing_store=self.backing_store_type,
        )

        # Now create the repository object that refers to the eden client
        self.repo = hgrepo.HgRepository(
            self.mount,
            system_hgrc=self.system_hgrc,
            filtered=self.backing_store_type == "filteredhg",
        )

    def edenfs_extra_config(self) -> Optional[Dict[str, List[str]]]:
        configs = super().edenfs_extra_config()
        if configs is None:
            configs = {}
        if (inode_catalog_type := self.inode_catalog_type) is not None:
            configs["overlay"] = [f'inode-catalog-type = "{inode_catalog_type}"']
        if self.enable_status_cache:
            configs["hg"] = ["enable-scm-status-cache = true"]
        return configs

    def create_backing_repo(self) -> hgrepo.HgRepository:
        if self.enable_windows_symlinks:
            init_configs = ["experimental.windows-symlinks=True"]
        else:
            init_configs = []
        hgrc = self.get_hgrc()
        repo = self.create_hg_repo("main", hgrc=hgrc, init_configs=init_configs)
        self.populate_backing_repo(repo)
        return repo

    def get_hgrc(self) -> configparser.ConfigParser:
        hgrc = get_default_hgrc()
        self.apply_hg_config_variant(hgrc)
        return hgrc

    def apply_hg_config_variant(self, hgrc: configparser.ConfigParser) -> None:
        hgrc["extensions"]["pushrebase"] = ""
        hgrc["remotefilelog"] = {
            "reponame": "eden_integration_tests",
            "cachepath": os.path.join(self.tmp_dir, "hgcache"),
        }

    @abc.abstractmethod
    def populate_backing_repo(self, repo: hgrepo.HgRepository) -> None:
        raise NotImplementedError(
            "individual test classes must implement " "populate_backing_repo()"
        )

    def hg(
        self,
        *args: str,
        encoding: str = "utf-8",
        input: Optional[str] = None,
        hgeditor: Optional[str] = None,
        cwd: Optional[str] = None,
        check: bool = True,
    ) -> str:
        """Runs `hg.real` with the specified args in the Eden mount.

        If hgeditor is specified, it will be used as the value of the $HGEDITOR
        environment variable when the hg command is run. See
        self.create_editor_that_writes_commit_messages().

        Returns the process stdout, as a string.

        The `encoding` parameter controls how stdout is decoded, and how the
        `input` parameter, if present, is encoded.
        """
        return self.repo.hg(
            *args,
            encoding=encoding,
            cwd=cwd,
            input=input,
            hgeditor=hgeditor,
            check=check,
        )

    def hg_clone_additional_repo(
        self,
        *clone_args: str,
        backing_repo: hgrepo.HgRepository,
        client_name: str = "repository",
    ) -> hgrepo.HgRepository:
        """Creates another Hg Repository using `hg clone`. This exercises a
        different code path than setup_eden_test(). This function returns two
        HgRepository objects. The first corresponds to the new Eden mount. The
        second corresponds to the backing repo (backed by a new eager repo)."""
        num_repos = len(self.adtl_repos)
        eager = str(Path(self.repos_dir) / f"eager_{num_repos}")
        mount = str(Path(self.mounts_dir) / f"{client_name}")

        # TODO: We rely on `hg clone` to create the eager repo for
        # us. We could theoretically provide our own to test more cases.
        cmd, env = FindExe.get_edenfsctl_env()
        self.repo.hg(
            "clone",
            f"eager:{eager}",
            f"{mount}",
            "--eden",
            "--config",
            "clone.use-rust=true",
            "--eden-backing-repo",
            f"{backing_repo.path}",
            "--config",
            f"edenfs.command={cmd}",
            "--config",
            f"edenfs.basepath={self.eden._base_dir}",
            *clone_args,
            cwd=self.mounts_dir,
            env=env,
        )

        # The use-eden-sparse config means that a FilteredFS repo was cloned
        is_filtered = False
        for a in clone_args:
            if a.startswith("clone.use-eden-sparse=") or a.startswith(
                "clone.eden-sparse-filter"
            ):
                is_filtered = True

        # Create the HgRepository objects for the new mount and backing repo
        mount = hgrepo.HgRepository(
            str(mount),
            system_hgrc=self.system_hgrc,
            filtered=is_filtered,
        )

        self.adtl_repos.append((mount, backing_repo))
        return mount

    def create_editor_that_writes_commit_messages(self, messages: List[str]) -> str:
        """
        Creates a program that writes the next message in `messages` to the
        file specified via $1 each time it is invoked.

        Returns the path to the program. This is intended to be used as the
        value for hgeditor in self.hg().
        """
        tmp_dir = self.tmp_dir

        messages_dir = os.path.join(tmp_dir, "commit_messages")
        os.makedirs(messages_dir)
        for i, message in enumerate(messages):
            file_name = "{:04d}".format(i)
            with open(os.path.join(messages_dir, file_name), "w") as f:
                f.write(message)

        editor = os.path.join(tmp_dir, "commit_message_editor")

        # Each time this script runs, it takes the "first" message file that is
        # left in messages_dir and moves it to overwrite the path that it was
        # asked to edit. This makes it so that the next time it runs, it will
        # use the "next" message in the queue.
        with open(editor, "w") as f:
            f.write(
                dedent(
                    f"""\
            #!/bin/bash
            set -e

            for entry in {messages_dir}/*
            do
                mv "$entry" "$1"
                exit 0
            done

            # There was no message to write.
            exit 1
            """
                )
            )
        os.chmod(editor, 0o755)
        return editor

    def assert_status(
        self,
        expected: Dict[str, str],
        msg: Optional[str] = None,
        op: Optional[str] = None,
        check_ignored: bool = True,
        rev: Optional[str] = None,
        timeout_seconds: float = 1.0,  # after adding status cache, we need to wait for edenfs to pick up the working copy modifications
    ) -> int:
        """Asserts the output of `hg status` matches the expected state.

        `expected` is a dict where keys are paths relative to the repo
        root and values are the single-character string that represents
        the status: 'M', 'A', 'R', '!', '?', 'I'.

        'C' is not currently supported.

        Use timeout to wait for EdenFS pick up the working copy modifications.
        For details of this see 'SyncBehavior' in eden.thrift

        Returns the total number of tries.
        """
        poll_interval_seconds = 0.1
        deadline = time.monotonic() + timeout_seconds
        num_of_tries = 0
        while True:
            try:
                num_of_tries += 1
                actual_status = self.repo.status(include_ignored=check_ignored, rev=rev)
                self.assertDictEqual(expected, actual_status, msg=msg)
                self.assert_unfinished_operation(op)
                break
            except AssertionError as e:
                if time.monotonic() >= deadline:
                    raise e
                time.sleep(poll_interval_seconds)
                continue
        return num_of_tries

    def assert_status_empty(
        self,
        msg: Optional[str] = None,
        op: Optional[str] = None,
        check_ignored: bool = True,
    ) -> None:
        """Ensures that `hg status` reports no modifications."""
        self.assert_status({}, msg=msg, op=op, check_ignored=check_ignored)

    def assert_unfinished_operation(self, op: Optional[str]) -> None:
        """
        Check if the repository appears to be in the middle of an unfinished
        update/rebase/graft/etc.

        The op argument should be the name fo the expected operation, or None
        to check that the repository is not in the middle of an unfinished
        operation.
        """
        # Ideally we could use `hg status` to detect if the repository is the
        # middle of an unfinished operation.  Unfortunately the built-in status
        # code provides no way to display that information when HGPLAIN is set.
        # There are also currently two copies of that code (in the morestatus
        # extension and built-in to the core status command), which
        # unfortunately do not check the same list of states.
        state_files = {
            "update": "updatestate",
            "updatemerge": "updatemergestate",
            "graft": "graftstate",
            "rebase": "rebasestate",
            "histedit": "histedit-state",
        }
        if not (op is None or op in state_files or op == "merge"):
            self.fail("invalid operation argument: %r" % (op,))

        for operation, state_file in state_files.items():
            state_path = os.path.join(self.repo.path, ".hg", state_file)
            in_state = os.path.exists(state_path)
            if in_state and operation != op:
                self.fail("repository is in the middle of an unfinished %s" % operation)
            elif not in_state and operation == op:
                self.fail(
                    "expected repository to be in the middle of an "
                    "unfinished %s, but it is not" % operation
                )

        # The merge state file is present when there are unresolved conflicts.
        # It may be present in addition to one of the unfinished state files
        # above.
        merge_state_path = os.path.join(self.repo.path, ".hg", "merge", "state2")
        in_merge = os.path.exists(merge_state_path)
        if in_merge and op is None:
            self.fail("repository is in the middle of an unfinished merge")
        elif op in {"updatemerge", "merge"} and not in_merge:
            self.fail(
                "expected repository to be in the middle of an "
                "unfinished merge, but it is not"
            )

    def assert_dirstate(
        self, expected: Dict[str, Tuple[str, int, str]], msg: Optional[str] = None
    ) -> None:
        """Asserts the output of `hg debugdirstate` matches the expected state.

        `expected` is a dict where keys are paths relative to the repo
        root and values are the expected dirstate tuples.  Each dirstate tuple
        is a 3-tuple consisting of (status, mode, merge_state)

        The `status` field is one of the dirstate status characters:
          'n', 'm', 'r', 'a', '?'

        The `mode` field should be the expected file permissions, as an integer.

        `merge_state` should be '' for no merge state, 'MERGE_OTHER', or
        'MERGE_BOTH'
        """
        output = self.hg("debugdirstate", "--json")
        data = json.loads(output)

        # Translate the json output into a dict that we can
        # compare with the expected dictionary.
        actual_dirstate = {}
        for path, entry in data.items():
            actual_dirstate[path] = (
                entry["status"],
                entry["mode"],
                entry["merge_state_string"],
            )

        self.assertDictEqual(expected, actual_dirstate, msg=msg)

    def assert_dirstate_empty(self, msg: Optional[str] = None) -> None:
        """Ensures that `hg debugdirstate` reports no entries."""
        self.assert_dirstate({}, msg=msg)

    def assert_copy_map(self, expected) -> None:
        stdout = self.eden.run_cmd("debug", "hg_copy_map_get_all", cwd=self.mount)
        observed_map = {}
        for line in stdout.split("\n"):
            if not line:
                continue
            src, dst = line.split(" -> ")
            observed_map[dst] = src
        self.assertEqual(expected, observed_map)

    def assert_unresolved(
        self,
        unresolved: Union[List[str], Set[str]],
        resolved: Optional[Union[List[str], Set[str]]] = None,
    ) -> None:
        out = self.hg("resolve", "--list")
        actual_resolved = set()
        actual_unresolved = set()
        for line in out.splitlines():
            status, path = line.split(None, 1)
            if status == "U":
                actual_unresolved.add(path)
            elif status == "R":
                actual_resolved.add(path)
            else:
                self.fail("unexpected entry in `hg resolve --list` output: %r" % line)

        self.assertEqual(actual_unresolved, set(unresolved))
        self.assertEqual(actual_resolved, set(resolved or []))

    def assert_file_regex(self, path: str, expected_regex, dedent: bool = True) -> None:
        if dedent:
            expected_regex = textwrap.dedent(expected_regex)
        contents = self.read_file(path)
        self.assertRegex(contents, expected_regex)

    def assert_journal(self, *entries: "JournalEntry") -> None:
        """
        Check that the journal contents match an expected state.

        Accepts a series of JournalEntry arguments, in order from oldest to
        newest expected journal entry.
        """
        data = self.repo.journal()
        failures = []

        # The 'hg journal' command returns entries from newest to oldest.
        # It feels a bit more logical in tests to list the entries from oldest
        # to newest (in the order in which we create them in the test), so
        # reverse the actual journal output when checking it.
        for idx, (expected, actual) in enumerate(
            itertools.zip_longest(entries, reversed(data))
        ):
            if actual is not None and expected is not None and expected.match(actual):
                # This entry matches
                continue

            if actual is None:
                formatted_actual = "None"
            else:
                formatted_actual = json.dumps(actual, indent=2, sort_keys=True)
                formatted_actual = "\n    ".join(formatted_actual.splitlines())
            failures.append(
                "journal mismatch at index %d:\n  expected: %s\n  actual=%s\n"
                % (idx, str(expected), formatted_actual)
            )

        if failures:
            self.fail("\n".join(failures))

    def assert_journal_empty(self) -> None:
        self.assertEqual([], self.repo.journal())


# Intended for use with any test that doesn't make sense to run on an
# unfiltered Hg repo. Examples are any test that applies filters to the repo.
class FilteredHgTestCase(EdenHgTestCase, metaclass=abc.ABCMeta):
    def setup_eden_test(self) -> None:
        self.backing_store_type = "filteredhg"
        super().setup_eden_test()


class JournalEntry:
    """
    JournalEntry describes an expected journal entry.
    It is intended to pass to EdenHgTestCase.assert_journal()
    """

    def __init__(self, command: str, name: str, old: str, new: str) -> None:
        """
        Create a JournalEntry object.

        The command argument only requires a regular expression match, rather
        than an exact string match.
        """
        self.command = command
        self.name = name
        self.old = old
        self.new = new

    def __str__(self) -> str:
        return (
            f"(command={self.command!r}, name={self.name!r}, "
            f"old={self.old!r}, new={self.new!r})"
        )

    def match(self, json_data: Dict[str, Any]) -> bool:
        user_command = self._strip_profiling_args(json_data["command"])
        if not re.search(self.command, user_command):
            return False
        if json_data["name"] != self.name:
            return False
        if json_data["oldhashes"] != [self.old]:
            return False
        if json_data["newhashes"] != [self.new]:
            return False
        return True

    def _strip_profiling_args(self, command: str) -> str:
        if command.startswith("--traceback "):
            command = command[len("--traceback ") :]

        # The hg wrapper randomly decides to profile a percentage of hg commands,
        # so it adds --profile and several other --config flags to the start of the
        # command arguments.
        #
        # These are unfortunately reported in the output of "hg journal", despite the
        # fact that this was not part of the command as originally invoked by the user.
        # Strip off these extra arguments to make sure that these extra arguments do not
        # interfere with our test checks.
        if not command.startswith("--profile --config 'profiling.type="):
            return command

        # The arguments added include a temporary file path, so they unfortunately are
        # not a fixed string.
        #
        # They are generally:
        #   --profile --config 'profiling.type=stat'
        #   --config 'profiling.output=[tmp_path]
        #   --config 'profiling.statformat=json'
        #   --config 'profiling.freq=50'
        #
        # Search for the last argument that gets added by the profiling code.
        m = re.search("--config 'profiling.freq=[0-9]+' ", command)
        if not m:
            logging.warning(
                "did not find match when trying to strip profiling "
                f"arguments: {command}"
            )
            return command

        # Remove all of the profiling arguments.
        # This is everything from the start of the command (we confirmed that --profile
        # was the first argument) up to and including the last profiling argument.
        return command[m.end() :]


MixinList = List[Tuple[str, List[Type[Any]]]]


def _replicate_hg_test(
    test_class: Type[EdenHgTestCase],
) -> Iterable[Tuple[str, Type[EdenHgTestCase]]]:
    tree_variants: MixinList = [("TreeOnly", [])]
    if eden.config.HAVE_NFS:
        tree_variants.append(("TreeOnlyNFS", [testcase.NFSTestMixin]))

    # Mix in FilteredHg tests if the build supports it.
    scm_variants: MixinList = [("", [])]
    if eden.config.HAVE_FILTEREDHG:
        scm_variants.append(("FilteredHg", [FilteredTestMixin]))

    overlay_variants: MixinList = [("", [])]
    if sys.platform == "win32":
        overlay_variants.append(("InMemory", [InMemoryOverlayTestMixin]))

    for tree_label, tree_mixins in tree_variants:
        for overlay_label, overlay_mixins in overlay_variants:
            for scm_label, scm_mixins in scm_variants:

                class VariantHgRepoTest(
                    *tree_mixins, *overlay_mixins, *scm_mixins, test_class
                ):
                    pass

                yield (
                    f"{tree_label}{overlay_label}{scm_label}",
                    typing.cast(Type[EdenHgTestCase], VariantHgRepoTest),
                )


def _replicate_filteredhg_test(
    test_class: Type[FilteredHgTestCase],
) -> Iterable[Tuple[str, Type[FilteredHgTestCase]]]:
    tree_variants: MixinList = [("TreeOnly", [])]
    if eden.config.HAVE_NFS:
        tree_variants.append(("TreeOnlyNFS", [testcase.NFSTestMixin]))

    for tree_label, tree_mixins in tree_variants:

        class VariantHgRepoTest(*tree_mixins, test_class):
            pass

        yield (
            f"{tree_label}",
            typing.cast(Type[FilteredHgTestCase], VariantHgRepoTest),
        )


def _replicate_status_cache_enabled_test(
    test_class: Type[FilteredHgTestCase],
) -> Iterable[Tuple[str, Type[FilteredHgTestCase]]]:
    """
    This takes whatever `_replicate_filteredhg_test` generates and adds
    another layer of variants for the status cache enabled/disabled.
    """
    cache_config_variants: MixinList = [
        ("WithStatusCacheDisabled", []),
        ("WithStatusCacheEnabled", [StatusCacheEnabledTestMixin]),
    ]
    for hg_test_label, hg_test_class in _replicate_hg_test(test_class):
        for cache_config_label, cache_config_mixins in cache_config_variants:

            class VariantHgRepoTest(*cache_config_mixins, hg_test_class):
                pass

            yield (
                f"{hg_test_label}{cache_config_label}",
                typing.cast(Type[FilteredHgTestCase], VariantHgRepoTest),
            )


class InMemoryOverlayTestMixin:
    inode_catalog_type = "inmemory"


class FilteredTestMixin:
    backing_store_type = "filteredhg"


class StatusCacheEnabledTestMixin:
    enable_status_cache = True


hg_test = testcase.test_replicator(_replicate_hg_test)
filteredhg_test = testcase.test_replicator(_replicate_filteredhg_test)
hg_cached_status_test = testcase.test_replicator(_replicate_status_cache_enabled_test)
