# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The existence, listing, and manifest properties: a created path is present, a removed
one absent, channels list the same entries, and manifest probes match the committed
tree. Complements the corpus families `existence` and `manifest`: these tests pin
when an existence or listing obligation exists at all, one rule per getattr evidence,
and manifest class representatives.
"""

from __future__ import annotations

import errno
import unittest

from eden.integration.correctness.exploratory.core.property import ObligationStatus
from eden.integration.correctness.exploratory.core.runspec import from_json
from eden.integration.correctness.exploratory.edenfs.contracts.checkout import (
    checkout_revisions,
)
from eden.integration.correctness.exploratory.edenfs.contracts.model import EdenPinned
from eden.integration.correctness.exploratory.edenfs.encoding import manifest_diff
from eden.integration.correctness.exploratory.edenfs.fixture import (
    generated_index,
    REPLACEMENT_STRIDE,
)
from eden.integration.correctness.exploratory.edenfs.target import EDENFS_TARGET
from eden.integration.correctness.exploratory.edenfs.test.judging import (
    judge,
    judge_isolated,
    obligations_of,
    status_of,
)
from eden.integration.correctness.exploratory.edenfs.test.testing import getattr_facts
from eden.integration.correctness.exploratory.test.corpus.author import (
    CHURN,
    errno_fail,
    NEW_DIR,
    R0,
    R1,
    Run,
    thrift_checkout,
    TIP,
)


ENOENT = 2


class ExistenceTest(unittest.TestCase):
    def test_reads_of_paths_that_are_never_directories_owe_no_directory_rule(
        self,
    ) -> None:
        run = Run("t", "read", "a read of r0")
        run.act("a", "r", "read", path=R0, channel="fuse")
        run.call("r", "fail", **errno_fail(ENOENT))
        run.finish()
        self.assertIsNone(
            status_of(
                judge_isolated(("existence",), run), "existence.directory-read", "r"
            )
        )

    def test_presence_is_judged_only_where_some_effect_rebinds_the_path(self) -> None:
        alone = Run("t", "alone", "a getattr of r0")
        alone.act("a", "g", "getattr", path=R0)
        alone.call("g", "ok", **getattr_facts())
        alone.finish()
        self.assertIsNone(
            status_of(
                judge_isolated(("existence",), alone),
                "existence.present-after-remove",
                "g",
            )
        )
        sl_rm = Run("t", "sl-rm", "sl rm r0, then a getattr")
        sl_rm.act("a", "h", "sl_rm", path=R0)
        sl_rm.act("a", "g", "getattr", path=R0)
        sl_rm.call("h", "ok", path=R0)
        sl_rm.call("g", "ok", **getattr_facts())
        sl_rm.finish()
        verdict = judge_isolated(("existence",), sl_rm)
        item = obligations_of(verdict)[("existence.present-after-remove", "g")]
        self.assertEqual(ObligationStatus.VIOLATED, item.status)
        self.assertEqual(
            [("observation-kind", "getattr")],
            [(w.role, w.value) for w in item.witnesses],
        )

    def test_both_absent_inside_a_rename_is_violated(self) -> None:
        run = Run("t", "both-absent", "r0 then r1 absent inside r0's rename onto r1")
        run.act("a", "u", "unlink", path=R1)
        run.act("a", "m", "rename", source=R0, destination=R1)
        run.act("b", "g", "getattr", path=R0)
        run.act("b", "h", "getattr", path=R1)
        run.call("u", "ok", path=R1)
        run.inv("m")
        run.call("g", "fail", **errno_fail(ENOENT, "stat"))
        run.call("h", "fail", **errno_fail(ENOENT, "stat"))
        run.done("m", "ok", source=R0, destination=R1)
        run.finish()
        item = obligations_of(judge_isolated(("existence",), run))[
            ("existence.rename-atomicity", "m")
        ]
        self.assertEqual(ObligationStatus.VIOLATED, item.status)
        self.assertIn(
            ("state", "both-absent"), [(w.role, w.value) for w in item.witnesses]
        )


class GetattrFilingTest(unittest.TestCase):
    def test_mkdir_type_files_each_channel_under_its_own_obligation(self) -> None:
        """A channel that disagrees with the other is not its co-observer."""
        for fuse, thrift, attribution in (
            ("directory", "file", {"g:thrift": "product"}),
            ("file", "directory", {"g": "product"}),
            ("file", "file", {"g": "undetermined", "g:thrift": "undetermined"}),
        ):
            with self.subTest(fuse=fuse, thrift=thrift):
                run = Run("t", "mkdir-type", "mkdir d, then a getattr of d")
                run.act("a", "m", "mkdir", path=NEW_DIR)
                run.act("a", "g", "getattr", path=NEW_DIR)
                run.call("m", "ok", path=NEW_DIR)
                run.call(
                    "g",
                    "ok",
                    **getattr_facts(fuse_type=fuse, thrift_type=thrift, mode=0o755),
                )
                run.finish()
                found = {
                    scope: str(item.attribution)
                    for (rule, scope), item in obligations_of(judge(run)).items()
                    if rule == "existence.mkdir-type"
                    and item.status == ObligationStatus.VIOLATED
                }
                self.assertEqual(attribution, found)


class ListingTest(unittest.TestCase):
    def test_a_pair_a_mkdir_may_disturb_is_no_obligation(self) -> None:
        names = ["r0.reg", "r1.reg", "r2.reg"]
        run = Run("t", "listing", "a mkdir between two listings")
        run.act("a", "f", "readdir", path="tree/registers", channel="fuse")
        run.act("b", "m", "mkdir", path=NEW_DIR)
        run.act("a", "t", "readdir", path="tree/registers", channel="thrift")
        run.inv("f")
        run.call("m", "ok", path=NEW_DIR)
        run.done("f", "ok", channel="fuse", entries=names)
        run.call("t", "ok", channel="thrift", entries=[*names[:2], "d", names[2]])
        run.finish()
        self.assertIsNone(
            status_of(
                judge_isolated(("existence", "listing"), run),
                "listing.channel-disagreement",
                "t",
            )
        )

    def _pair(self, nemesis: str | None) -> Run:
        """Two disagreeing listings of `tree/registers`, maybe inside a nemesis window."""
        names = ["r0.reg", "r1.reg", "r2.reg"]
        run = Run("t", f"listing-{nemesis}", "two listings with a nemesis over them")
        if nemesis == "fault":
            run.act(
                "nemesis",
                "f",
                "fault_install",
                fault_id="F",
                site="overlay-write",
                target_action="w",
                behavior="error",
            )
            run.act(
                "b",
                "w",
                "write",
                path=CHURN,
                value_id="w",
                requires=[("f", "finished")],
            )
            run.act(
                "nemesis",
                "x",
                "fault_remove",
                fault_id="F",
                requires=[("w", "finished")],
            )
            run.call("f", "ok", fault_id="F")
        if nemesis == "kill":
            run.act("b", "s", "status", channel="sl")
            run.act(
                "nemesis",
                "k",
                "kill_process",
                target_action="s",
                after_ms=100,
                requires=[("s", "started")],
            )
            run.inv("s", "k")
        run.act("a", "lf", "readdir", path="tree/registers", channel="fuse")
        run.act("a", "lt", "readdir", path="tree/registers", channel="thrift")
        run.call("lf", "ok", channel="fuse", entries=names)
        run.call("lt", "ok", channel="thrift", entries=names[:2])
        if nemesis == "fault":
            run.call("w", "fail", **errno_fail(errno.EIO))
            run.call("x", "ok", fault_id="F", removed=1)
            run.fault_removed("F")
        if nemesis == "kill":
            run.done("k", "ok", target_action="s", pgid=4242, group_empty=True)
            run.done(
                "s",
                "fail",
                reason="status: killed by signal 9",
                returncode=-9,
                stdout="",
                stderr="",
            )
        run.finish()
        return run

    def test_a_nemesis_writing_no_entry_does_not_disturb_the_pair(self) -> None:
        """A fault lifetime or a kill window over the pair writes no entry of the
        directory, so the pair is still compared."""
        for nemesis in (None, "fault", "kill"):
            with self.subTest(nemesis=nemesis):
                verdict = judge_isolated(("existence", "listing"), self._pair(nemesis))
                self.assertEqual(
                    "violated",
                    status_of(verdict, "listing.channel-disagreement", "lt"),
                )


class ManifestTest(unittest.TestCase):
    def test_a_probe_listing_other_paths_is_not_evaluated(self) -> None:
        run = Run("t", "manifest", "a probe listing a register")
        run.act("a", "p", "manifest_presence", from_revision=TIP, to_revision=2)
        run.call(
            "p",
            "ok",
            deleted_presence=[{"path": R0, "present": False}],
            added_presence=[],
        )
        run.finish()
        item = obligations_of(judge_isolated(("existence", "manifest"), run))[
            ("manifest.checkout-mismatch", "p")
        ]
        self.assertEqual(ObligationStatus.NOT_EVALUATED, item.status)
        self.assertEqual("manifest.unexpected-paths", item.not_evaluated_reason)

    def test_class_members_map_to_their_representative(self) -> None:
        run = Run("t", "classes", "a probe")
        run.act("a", "p", "manifest_presence", from_revision=TIP, to_revision=2)
        spec = from_json(run.spec_text(), EDENFS_TARGET)
        pinned = EdenPinned.for_spec(spec)
        fixture = pinned.fixture
        revisions = checkout_revisions(spec, pinned)
        classes = fixture.classes(revisions)
        member = fixture.layout.file_at(0, fixture.initial_revision).path
        mapped = fixture.representatives([member, R0], revisions)
        self.assertNotIn(R0, mapped)
        self.assertIn(mapped[member], {item.representative for item in classes})
        self.assertEqual(
            {item.representative: item.representative for item in classes},
            fixture.representatives(
                [item.representative for item in classes], revisions
            ),
        )

    def _unvisited(self, planted: bool, checkouts: tuple[int, ...]) -> Run:
        """A probe from rev0, which no checkout visits, to the tip, after `checkouts`.

        Old paths replaced at rev1 are absent at every revision the run
        can reach.
        """
        run = Run("t", "manifest", "a probe from an unvisited revision")
        deleted, added, _changed = manifest_diff(run.layout(), 0, TIP)
        for index, revision in enumerate(checkouts):
            run.act(
                "a",
                f"c{index}",
                "checkout",
                revision=revision,
                via="thrift",
                mode="normal",
            )
        run.act("a", "p", "manifest_presence", from_revision=0, to_revision=TIP)
        for index, revision in enumerate(checkouts):
            run.call(f"c{index}", "ok", **thrift_checkout(revision, conflicts=0))
        never = min(deleted, key=lambda path: (_replaced_at(run, path), path))
        run.call(
            "p",
            "ok",
            deleted_presence=[
                {"path": path, "present": planted and path == never}
                for path in sorted(deleted)
            ],
            added_presence=[{"path": path, "present": True} for path in sorted(added)],
        )
        run.finish()
        return run

    def test_paths_removed_before_every_reachable_revision_are_judged(self) -> None:
        for checkouts in ((), (1, TIP)):
            for planted, expected in ((False, "satisfied"), (True, "violated")):
                with self.subTest(checkouts=checkouts, planted=planted):
                    run = self._unvisited(planted, checkouts)
                    item = obligations_of(
                        judge_isolated(("existence", "manifest"), run)
                    )[("manifest.checkout-mismatch", "p")]
                    self.assertEqual(expected, str(item.status))

    def test_never_present_paths_share_one_representative(self) -> None:
        run = Run("t", "classes", "a probe from rev0")
        run.act("a", "c", "checkout", revision=2, via="thrift", mode="normal")
        run.act("a", "p", "manifest_presence", from_revision=0, to_revision=TIP)
        spec = from_json(run.spec_text(), EDENFS_TARGET)
        fixture = EdenPinned.for_spec(spec).fixture
        layout = fixture.layout
        revisions = checkout_revisions(spec, EdenPinned.for_spec(spec))
        self.assertEqual((2, TIP), revisions)
        # Index 0 (executable) and 1 are replaced at rev1 and rev2; index 2 at rev3.
        gone = [layout.file_at(index, 0).path for index in (0, 1)]
        kept = layout.file_at(2, 0).path
        mapped = fixture.representatives([*gone, kept], revisions)
        self.assertEqual({gone[0]}, {mapped[path] for path in gone})
        self.assertNotEqual(gone[0], mapped[kept])
        self.assertEqual(
            {gone[1]: gone[1]},
            fixture.representatives(gone, revisions, frozenset({gone[0]})),
        )


def _replaced_at(run: Run, path: str) -> int:
    index = generated_index(run.layout(), path)
    assert index is not None
    return index % REPLACEMENT_STRIDE + 1
