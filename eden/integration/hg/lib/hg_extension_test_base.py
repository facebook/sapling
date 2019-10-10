#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import abc
import configparser
import itertools
import json
import logging
import os
import re
import textwrap
from textwrap import dedent
from typing import Any, Dict, List, Optional, Set, Tuple, Union

from eden.integration.lib import hgrepo, testcase


def get_default_hgrc() -> configparser.ConfigParser:
    """
    Get the default hgrc settings to use in the backing store repository.

    This returns the base settings, which can then be further adjusted by test
    cases and test case variants.
    """
    hgrc = configparser.ConfigParser()
    # TODO(mbolin): This is supposed to replace experimental.updatecheck,
    # but it does not appear to be taking effect today. The
    # experimental.updatecheck setting on this hgrc should be removed once
    # it has been deprecated and update.check does what it is supposed to
    # do.
    hgrc["commands"] = {"update.check": "noconflict"}
    hgrc["ui"] = {
        "origbackuppath": ".hg/origbackups",
        "username": "Kevin Flynn <lightcyclist@example.com>",
    }
    hgrc["experimental"] = {
        "evolution": "createmarkers",
        "evolutioncommands": "prev next split fold obsolete metaedit",
        "updatecheck": "noconflict",
    }
    hgrc["extensions"] = {
        "absorb": "",
        "amend": "",
        "directaccess": "",
        "fbhistedit": "",
        "histedit": "",
        "purge": "",
        "rebase": "",
        "reset": "",
        "strip": "",
        "tweakdefaults": "",
        "undo": "",
    }
    hgrc["directaccess"] = {"loadsafter": "tweakdefaults"}
    return hgrc


class EdenHgTestCase(testcase.EdenTestCase, metaclass=abc.ABCMeta):
    """
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
    """

    repo: hgrepo.HgRepository
    backing_repo: hgrepo.HgRepository
    backing_repo_name: str
    config_variant_name: str  # set by the @hg_test decorator

    def setup_eden_test(self) -> None:
        super().setup_eden_test()

        # Create the backing repository
        self.backing_repo = self.create_backing_repo()

        self.backing_repo_name = "backing_repo"
        self.eden.add_repository(self.backing_repo_name, self.backing_repo.path)
        # Edit the edenrc file to set up post-clone hooks that will correctly
        # populate the .hg directory inside the eden client.
        self.eden.clone(self.backing_repo_name, self.mount, allow_empty=True)

        # Now create the repository object that refers to the eden client
        self.repo = hgrepo.HgRepository(self.mount, system_hgrc=self.system_hgrc)

    def create_backing_repo(self) -> hgrepo.HgRepository:
        hgrc = self.get_hgrc()
        repo = self.create_hg_repo("main", hgrc=hgrc)
        self.populate_backing_repo(repo)
        return repo

    def get_hgrc(self) -> configparser.ConfigParser:
        hgrc = get_default_hgrc()
        self.apply_hg_config_variant(hgrc)
        return hgrc

    @abc.abstractmethod
    def apply_hg_config_variant(self, hgrc: configparser.ConfigParser) -> None:
        raise NotImplementedError()

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
    ) -> None:
        """Asserts the output of `hg status` matches the expected state.

        `expected` is a dict where keys are paths relative to the repo
        root and values are the single-character string that represents
        the status: 'M', 'A', 'R', '!', '?', 'I'.

        'C' is not currently supported.
        """
        actual_status = self.repo.status(include_ignored=check_ignored)
        self.assertDictEqual(expected, actual_status, msg=msg)
        self.assert_unfinished_operation(op)

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
        merge_state_path = os.path.join(self.repo.path, ".hg", "merge", "state")
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
    ):
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

    def assert_dirstate_empty(self, msg: Optional[str] = None):
        """Ensures that `hg debugdirstate` reports no entries."""
        self.assert_dirstate({}, msg=msg)

    def assert_copy_map(self, expected):
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

    def assert_file_regex(self, path, expected_regex, dedent=True):
        if dedent:
            expected_regex = textwrap.dedent(expected_regex)
        contents = self.read_file(path)
        self.assertRegex(contents, expected_regex)

    def assert_journal(self, *entries: "JournalEntry") -> None:
        """
        Check that the journal contents match an expected state.

        Acceptes a series of JournalEntry arguments, in order from oldest to
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


class JournalEntry(object):
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
            logging.warn(
                "did not find match when trying to strip profiling "
                f"arguments: {command}"
            )
            return command

        # Remove all of the profiling arguments.
        # This is everything from the start of the command (we confirmed that --profile
        # was the first argument) up to and including the last profiling argument.
        return command[m.end() :]


def _apply_treemanifest_config(test, config):
    config["extensions"]["fastmanifest"] = ""
    config["extensions"]["treemanifest"] = ""
    config["extensions"]["pushrebase"] = ""
    config["fastmanifest"] = {
        "usetree": "True",
        "usecache": "False",
        "cacheonchange": "True",
    }
    config["remotefilelog"] = {
        "reponame": "eden_integration_tests",
        "cachepath": os.path.join(test.tmp_dir, "hgcache"),
    }


def _apply_treeonly_config(test, config):
    config["extensions"]["treemanifest"] = ""
    config["treemanifest"] = {"treeonly": "True"}
    config["remotefilelog"] = {
        "reponame": "eden_integration_tests",
        "cachepath": os.path.join(test.tmp_dir, "hgcache"),
    }


ALL_CONFIGS = {
    "Treemanifest": _apply_treemanifest_config,
    "TreeOnly": _apply_treeonly_config,
}


def _replicate_hg_test(test_class, *variants):
    if not variants:
        variants = ("Treemanifest",)

    for name in variants:
        config_fn = ALL_CONFIGS[name]

        # pyre-fixme[31]: Expression `test_class` is not a valid type.
        class HgTestVariant(test_class):
            config_variant_name = name
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
