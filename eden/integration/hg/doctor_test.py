#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

import json
import os
import shlex
import subprocess
from pathlib import Path

from eden.fs.service.eden.thrift_types import (
    ResetParentCommitsParams,
    WorkingDirectoryParents,
)
from eden.integration.lib import hgrepo

from .lib.hg_extension_test_base import EdenHgTestCase, hg_test


@hg_test
# pyre-ignore[13]: T62487924
class DoctorTest(EdenHgTestCase):
    # pyre-fixme[13]: Attribute `commit1` is never initialized.
    commit1: str
    # pyre-fixme[13]: Attribute `commit2` is never initialized.
    commit2: str

    def populate_backing_repo(self, repo: hgrepo.HgRepository) -> None:
        repo.write_file("letters", "a\nb\nc\n")
        repo.write_file("numbers", "1\n2\n3\n")
        self.commit1 = repo.commit("Initial commit.")

        repo.write_file("letters", "a\n")
        repo.write_file("numbers", "1\n")
        self.commit2 = repo.commit("New commit.")

    def run_doctor(self, dry_run: bool) -> subprocess.CompletedProcess:
        args = ["doctor", "--current-edenfs-only"]
        if dry_run:
            args.append("-n")
        return self.eden.run_unchecked(*args, stdout=subprocess.PIPE)

    def test_eden_doctor_doesnt_crash_on_merge_commit(self) -> None:
        self.repo.write_file("hello.txt", "one")
        commit3 = self.repo.commit("Commit three")

        self.repo.update(self.commit2)
        self.repo.write_file("hello.txt", "two")
        commit4 = self.repo.commit("Commit four")

        with self.assertRaises(hgrepo.HgError):
            self.hg("rebase", "-r", commit4, "-d", commit3)
        self.assert_unresolved(unresolved=["hello.txt"])

        cmd_result = self.run_doctor(dry_run=False)
        self.assertNotIn(b"AssertionError", cmd_result.stdout)

    def get_eden_parent(self) -> str:
        data = self.eden.run_cmd("debug", "parents", cwd=self.mount)
        return data.rstrip()

    async def test_eden_doctor_fixes_valid_mismatched_parents(self) -> None:
        # this test is only meaningful when the default behavior of fixing the
        # dirstate is disabled
        self.hg("config", "--local", "experimental.repair-eden-dirstate", "False")

        # this specifically tests when EdenFS and Mercurial are out of sync,
        # but and mercurial does know about EdenFS's WCP
        mount_path = Path(self.mount)

        # set eden to point at the first commit, while keeping mercurial at the
        # second commit
        parents = WorkingDirectoryParents(parent1=self.commit1.encode("utf-8"))
        params = ResetParentCommitsParams()
        async with self.eden.get_async_thrift_client() as client:
            await client.resetParentCommits(
                mountPoint=bytes(mount_path), parents=parents, params=params
            )

        with self.assertRaises(hgrepo.HgError) as status_context:
            self.repo.status()

        self.assertIn(
            b"requested parent commit is out-of-date", status_context.exception.stderr
        )

        eden_parent = self.get_eden_parent()
        hg_parent = self.hg("log", "-r.", "-T{node}")

        # make sure that eden and mercurial are out of sync
        self.assertNotEqual(eden_parent, hg_parent)

        cmd_result = self.run_doctor(dry_run=True)
        error_msg = (
            "mercurial's parent commit is %s, but Eden's internal parent commit is %s"
            % (self.commit2, self.commit1)
        )
        self.assertIn(error_msg.encode("utf-8"), cmd_result.stdout)

        # run eden doctor and make sure eden and mercurial are in sync again
        fixed_result = self.run_doctor(dry_run=False)
        self.assertIn(b"Successfully fixed 1 problem", fixed_result.stdout)

        eden_parent_fixed = self.get_eden_parent()
        hg_parent_fixed = self.hg("log", "-r.", "-T{node}")
        self.assertEqual(eden_parent_fixed, hg_parent_fixed)

        # Since Eden's snapshot file pointed to a known commit, it should pick
        # Eden's parent as the new parent
        self.assertEqual(eden_parent, hg_parent_fixed)

    def test_eden_doctor_fixes_bad_dirstate_file(self) -> None:
        repo_path = Path(self.repo.path)
        # replace .hg/dirstate with an empty file
        (repo_path / ".hg" / "dirstate").write_bytes(b"")

        cmd_result = self.run_doctor(dry_run=True)
        doctor_out = cmd_result.stdout.decode("utf-8")
        self.assertIn(
            f"Found inconsistent/missing data in {repo_path / '.hg'}", doctor_out
        )
        self.assertIn(f"error parsing .hg{os.sep}dirstate", doctor_out)

        fixed_result = self.run_doctor(dry_run=False)
        self.assertIn(b"Successfully fixed 1 problem", fixed_result.stdout)

        fixed_parent = self.hg("log", "-r.", "-T{node}")
        self.assertEqual(fixed_parent, self.commit2)

    def test_doctor_watchman_error_reply(self) -> None:
        # A watchman whose `watch-project` fails answers with an error, not
        # with a watcher. `eden doctor` skips the watcher check instead of
        # reporting a watcher of the wrong type.
        fake_bin = Path(self.tmp_dir) / "fake_watchman_bin"
        fake_bin.mkdir()
        script = fake_bin / "watchman"
        watch_list_reply = shlex.quote(json.dumps({"roots": [self.mount]}))
        script.write_text(
            "#!/bin/sh\n"
            'case "$1" in\n'
            f"  watch-list) printf '%s' {watch_list_reply} ;;\n"
            "  *) echo 'watch-project failed' >&2; exit 1 ;;\n"
            "esac\n"
        )
        script.chmod(0o755)
        env = {"PATH": f"{fake_bin}:{os.environ['PATH']}"}
        result = self.eden.run_unchecked(
            "doctor", "--current-edenfs-only", "-n", stdout=subprocess.PIPE, env=env
        )
        out = result.stdout.decode()
        self.assertNotIn("wrong watcher type", out)
        self.assertNotIn("Would fix watchman watch", out)
