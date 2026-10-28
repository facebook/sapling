# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import annotations

import functools
import hashlib
import itertools
import json
import math
import re
import unittest
from typing import Any, Callable, cast

from eden.integration.correctness.exploratory.core.effects import (
    ABSENT,
    Agree,
    ANY,
    Atom,
    AtomOp,
    Before,
    Cell,
    constraint_id,
    Count,
    Domain,
    Effect,
    EffectGroup,
    EffectKind,
    EffectSet,
    eq,
    Exclusive,
    Expect,
    Footprint,
    footprints_interact,
    Guard,
    is_canonical_key,
    MustAppearIn,
    none_of,
    normalize_path,
    ObligationRef,
    Observation,
    of_class,
    one_of,
    ProofCertificate,
    read_proof,
    Requirement,
    ROOT,
    Status,
    Supersedes,
    UnknownInitial,
    Value,
    VisibleAfterAck,
    Window,
)

_REF = ObligationRef("bytes.stale-read", "r1")
_A = Cell("primary", "path", "a")
_B = Cell("primary", "path", "b")
_C = Cell("primary", "path", "c")
_FILE = Value("file", ("blob", 3))


def _produce(cell: Cell = _A, value: Value = _FILE) -> Effect:
    return Effect(cell, EffectKind.PRODUCE, value, None, "w1")


def _group(**overrides: Any) -> EffectGroup:
    fields: dict[str, Any] = {
        "id": "g1",
        "action": "w1",
        "status": Status.ACKED,
        "window": Window(1.0, 2.0),
        "effects": (_produce(),),
    }
    return EffectGroup(**{**fields, **overrides})


def _observation(**overrides: Any) -> Observation:
    fields: dict[str, Any] = {
        "id": "o1",
        "action": "r1",
        "cells": (_A,),
        "expect": Expect(((eq(_FILE),),)),
        "window": Window(3.0, 4.0),
        "channel": "fuse",
        "obligations": (_REF,),
    }
    return Observation(**{**fields, **overrides})


def _bad(value: object) -> Any:
    return cast(Any, value)


def _rejects(test: unittest.TestCase, message: str, build: Callable[[], Any]) -> None:
    with test.subTest(message=message):
        with test.assertRaisesRegex(ValueError, re.escape(message)):
            build()


class FootprintTest(unittest.TestCase):
    def test_paths_normalize_and_escapes_reject(self) -> None:
        self.assertEqual("a/b", normalize_path("./a//b/"))
        self.assertEqual(ROOT, normalize_path("."))
        footprint = Footprint(
            reads=frozenset({"a/./b"}),
            writes=frozenset({"c//"}),
            subtrees_read=frozenset({""}),
            subtrees_written=frozenset({"d/"}),
            channels=frozenset({"fuse"}),
        )
        self.assertEqual(
            {
                "reads": ["a/b"],
                "writes": ["c"],
                "subtrees_read": [""],
                "subtrees_written": ["d"],
                "channels": ["fuse"],
            },
            footprint.document(),
        )
        self.assertEqual(
            Footprint(reads=frozenset({"a/b"})), Footprint(reads=frozenset({"a//b"}))
        )
        _rejects(self, "is absolute", lambda: Footprint(reads=frozenset({"/a"})))
        _rejects(
            self,
            "escapes the checkout",
            lambda: Footprint(writes=frozenset({"a/../.."})),
        )
        _rejects(self, "must be a string", lambda: Footprint(reads=_bad({1})))
        _rejects(self, "not a string", lambda: Footprint(reads=_bad("ab")))
        _rejects(
            self,
            "bad footprint channel",
            lambda: Footprint(channels=frozenset({"Fuse"})),
        )


class FootprintInteractionTest(unittest.TestCase):
    def test_a_write_interacts_with_what_names_its_path(self) -> None:
        write = Footprint(writes=frozenset({"a/b"}))
        cases = {
            "same exact read": (Footprint(reads=frozenset({"a/b"})), True),
            "same exact write": (Footprint(writes=frozenset({"a/b"})), True),
            "subtree read above": (Footprint(subtrees_read=frozenset({"a"})), True),
            "root subtree write": (Footprint(subtrees_written=frozenset({""})), True),
            "sibling read": (Footprint(reads=frozenset({"a/c"})), False),
            "prefix-named sibling subtree": (
                Footprint(subtrees_read=frozenset({"a/bc"})),
                False,
            ),
            "subtree read below": (
                Footprint(subtrees_read=frozenset({"a/b/c"})),
                False,
            ),
            "nothing": (Footprint(), False),
        }
        for name, (other, expected) in cases.items():
            with self.subTest(name=name):
                self.assertEqual(expected, footprints_interact(write, other))
                self.assertEqual(expected, footprints_interact(other, write))

    def test_a_subtree_write_covers_paths_above_and_below(self) -> None:
        moved = Footprint(subtrees_written=frozenset({"a/b"}))
        for other, expected in (
            (Footprint(reads=frozenset({"a/b/c"})), True),
            (Footprint(subtrees_read=frozenset({"a"})), True),
            (Footprint(reads=frozenset({"a"})), True),
            (Footprint(reads=frozenset({"a/c"})), False),
        ):
            with self.subTest(other=other):
                self.assertEqual(expected, footprints_interact(moved, other))

    def test_readers_never_interact(self) -> None:
        self.assertFalse(
            footprints_interact(
                Footprint(subtrees_read=frozenset({""})),
                Footprint(reads=frozenset({"a"}), subtrees_read=frozenset({""})),
            )
        )


class ValueTest(unittest.TestCase):
    def test_subjects_values_and_windows_validate(self) -> None:
        self.assertEqual(["primary", "path", "a"], _A.document())
        self.assertEqual({"cls": "absent", "data": []}, ABSENT.document())
        nested = Value("dir", (("a", 1), None, True))
        self.assertEqual(
            {"cls": "dir", "data": [["a", 1], None, True]}, nested.document()
        )
        self.assertEqual([1.0, None], Window(1.0, math.inf).document())
        self.assertEqual([1, 1], Window(1, 1).document())
        _rejects(self, "bad cell side", lambda: Cell("mirror", "path", "a"))
        _rejects(self, "bad cell space", lambda: Cell("primary", "Path", "a"))
        _rejects(self, "bad cell key", lambda: Cell("primary", "path", _bad(1)))
        _rejects(self, "bad value class", lambda: Value("File"))
        _rejects(self, "value data must be a tuple", lambda: Value("file", _bad(["a"])))
        _rejects(self, "got float", lambda: Value("file", (1.5,)))
        _rejects(
            self,
            "nests too deeply",
            lambda: Value("file", ((((((((((1,),),),),),),),),),)),
        )
        _rejects(self, "window lo must be finite", lambda: Window(-math.inf, 1.0))
        _rejects(self, "window hi must be a number", lambda: Window(1.0, math.nan))
        _rejects(self, "is empty", lambda: Window(2.0, 1.0))


class EffectTest(unittest.TestCase):
    def test_effects_and_groups_validate(self) -> None:
        transfer = Effect(_B, EffectKind.TRANSFER, None, _A, "mv")
        removal = _produce(_C, value=ABSENT)
        group = _group(effects=(transfer, removal))
        self.assertEqual("g1", group.outcome_var)
        self.assertEqual("shared", _group(outcome_var="shared").outcome_var)
        self.assertEqual(
            {
                "id": "g1",
                "action": "w1",
                "status": "acked",
                "window": [1.0, 2.0],
                "outcome_var": "g1",
                "effects": [
                    {
                        "cell": ["primary", "path", "b"],
                        "kind": "transfer",
                        "value": None,
                        "source": ["primary", "path", "a"],
                        "provenance": "mv",
                        "guard": None,
                    },
                    {
                        "cell": ["primary", "path", "c"],
                        "kind": "produce",
                        "value": {"cls": "absent", "data": []},
                        "source": None,
                        "provenance": "w1",
                        "guard": None,
                    },
                ],
                "requires": [],
            },
            group.document(),
        )
        reference = Cell("reference", "path", "a")
        _rejects(
            self,
            "a produce effect has a value and no source",
            lambda: Effect(_A, EffectKind.PRODUCE, None, None, "w"),
        )
        _rejects(
            self,
            "a transfer effect has a source and no value",
            lambda: Effect(_B, EffectKind.TRANSFER, _FILE, _A, "mv"),
        )
        _rejects(
            self,
            "distinct cells on one side",
            lambda: Effect(_A, EffectKind.TRANSFER, None, _A, "mv"),
        )
        _rejects(
            self,
            "distinct cells on one side",
            lambda: Effect(_A, EffectKind.TRANSFER, None, reference, "mv"),
        )
        _rejects(
            self, "bad effect kind", lambda: Effect(_A, _bad("copy"), _FILE, None, "w")
        )
        _rejects(self, "non-empty tuple of effects", lambda: _group(effects=()))
        _rejects(
            self,
            "spans sides",
            lambda: _group(effects=(_produce(), _produce(cell=reference))),
        )
        _rejects(self, "bad group status", lambda: _group(status="maybe"))
        _rejects(self, "bad effect group id", lambda: _group(id=""))


class ExpectationTest(unittest.TestCase):
    def test_atoms_match_exactly_their_operands(self) -> None:
        dir_value = Value("dir")
        cases = (
            (ANY, (_FILE, ABSENT), ()),
            (eq(_FILE), (_FILE,), (ABSENT,)),
            (one_of({_FILE, ABSENT}), (_FILE, ABSENT), (dir_value,)),
            (none_of({ABSENT}), (_FILE, dir_value), (ABSENT,)),
            (of_class({"file"}), (_FILE, Value("file")), (ABSENT,)),
        )
        for atom, matching, other in cases:
            with self.subTest(op=atom.op):
                self.assertTrue(all(atom.matches(value) for value in matching))
                self.assertFalse(any(atom.matches(value) for value in other))
        _rejects(self, "eq atom has the wrong operands", lambda: Atom(AtomOp.EQ))
        _rejects(
            self,
            "names exactly one value",
            lambda: Atom(AtomOp.EQ, frozenset({_FILE, ABSENT})),
        )
        _rejects(
            self,
            "any atom has the wrong operands",
            lambda: Atom(AtomOp.ANY, frozenset({_FILE})),
        )
        _rejects(self, "class atom has the wrong operands", lambda: of_class(()))
        _rejects(self, "bad atom class", lambda: of_class({"File"}))
        _rejects(self, "must be frozensets", lambda: Atom(AtomOp.IN, _bad({_FILE})))

    def test_counts_match_brute_force(self) -> None:
        """A count holds exactly when per-cell amounts within the set cells'
        weights reach the total."""
        weights = ((1, 1), (2, 2), (0, 3), (1, 2))
        for values in itertools.product((_FILE, ABSENT), repeat=len(weights)):
            reachable = {0}
            for (low, high), value in zip(weights, values):
                if value == _FILE:
                    reachable = {
                        total + amount
                        for total in reachable
                        for amount in range(low, high + 1)
                    }
            for total in range(-1, 10):
                with self.subTest(values=values, total=total):
                    expect = Expect(count=Count(_FILE, weights, total))
                    self.assertEqual(total in reachable, expect.holds(values))
        self.assertEqual(4, Expect(count=Count(_FILE, weights, 0)).width)
        _rejects(
            self,
            "a count expectation has no rows",
            lambda: Expect(((ANY,),), Count(_FILE, ((1, 1),), 1)),
        )
        _rejects(self, "0 <= low <= high", lambda: Count(_FILE, ((2, 1),), 1))
        _rejects(self, "0 <= low <= high", lambda: Count(_FILE, (), 0))

    def test_expectations_and_observations_validate(self) -> None:
        expect = Expect(((eq(_FILE), ANY), (eq(ABSENT), eq(ABSENT))))
        self.assertTrue(expect.holds((_FILE, ABSENT)))
        self.assertTrue(expect.holds((ABSENT, ABSENT)))
        self.assertFalse(expect.holds((ABSENT, _FILE)))
        observation = _observation(cells=(_A, _B), expect=expect)
        self.assertEqual(
            [["bytes.stale-read", "r1"]], observation.document()["obligations"]
        )
        reordered = Expect(((one_of({ABSENT, _FILE}),),))
        self.assertEqual(
            json.dumps(reordered.document()),
            json.dumps(Expect(((one_of({_FILE, ABSENT}),),)).document()),
        )
        _rejects(self, "at least one row", lambda: Expect(()))
        _rejects(self, "one width", lambda: Expect(((ANY,), (ANY, ANY))))
        _rejects(self, "tuple of atoms", lambda: Expect(_bad(([ANY],))))
        _rejects(
            self,
            "rows constrain 1 cells, not 2",
            lambda: _observation(cells=(_A, _B)),
        )
        _rejects(
            self,
            "repeats a cell",
            lambda: _observation(cells=(_A, _A), expect=expect),
        )
        _rejects(self, "non-empty tuple of cells", lambda: _observation(cells=()))
        _rejects(self, "bad observation channel", lambda: _observation(channel="FUSE"))
        _rejects(
            self,
            "non-empty tuple of ObligationRefs",
            lambda: _observation(obligations=_bad(("bytes.stale-read", "r1"))),
        )
        _rejects(
            self,
            "non-empty tuple of ObligationRefs",
            lambda: _observation(obligations=()),
        )
        _rejects(
            self, "bad obligation scope", lambda: ObligationRef("bytes.stale-read", "")
        )


class ConstraintTest(unittest.TestCase):
    def test_constraints_encode_their_precedence(self) -> None:
        self.assertEqual(
            Before("g1", "g2", _REF), Supersedes("g1", "g2", _REF).as_before()
        )
        self.assertEqual(
            Before("o1", "g1", _REF), MustAppearIn("g1", "o1", _REF).as_before()
        )
        self.assertEqual(
            Before("g1", "o1", _REF), VisibleAfterAck("g1", "o1", _REF).as_before()
        )
        self.assertEqual(("o1", "o2"), Agree("o1", "o2", "sorted-names", _REF).nodes())
        before = Before("a", "b", _REF)
        self.assertEqual(
            "sha256:"
            + hashlib.sha256(
                json.dumps(
                    before.document(), sort_keys=True, separators=(",", ":")
                ).encode()
            ).hexdigest(),
            constraint_id(before),
        )
        self.assertEqual(
            {
                "kind": "agree",
                "nodes": ["o1", "o2"],
                "obligation": ["bytes.stale-read", "r1"],
                "normalize": "sorted-names",
            },
            Agree("o1", "o2", "sorted-names", _REF).document(),
        )
        _rejects(self, "relates 'a' to itself", lambda: Before("a", "a", _REF))
        _rejects(self, "bad constraint node", lambda: VisibleAfterAck("", "o1", _REF))
        _rejects(self, "bad normalization id", lambda: Agree("o1", "o2", "Raw", _REF))
        _rejects(self, "not an ObligationRef", lambda: Before("a", "b", _bad("ref")))


class EffectSetTest(unittest.TestCase):
    def test_initial_values_are_queried_lazily_once(self) -> None:
        asked: list[Cell] = []

        class Pinned:
            def initial(self, cell: Cell) -> Value:
                asked.append(cell)
                return ABSENT if cell == _B else _FILE

        effects = EffectSet((_group(),), Pinned(), (Domain("primary", "path", "d"),))
        self.assertEqual([], asked)
        self.assertEqual(_FILE, effects.initial(_A))
        self.assertEqual(_FILE, effects.initial(_A))
        self.assertEqual(ABSENT, effects.initial(_B))
        self.assertEqual([_A, _B], asked)
        self.assertEqual(frozenset({"g1"}), effects.group_ids)
        self.assertEqual([["primary", "path", "d"]], effects.document()["domains"])

        class Broken:
            def initial(self, cell: Cell) -> Any:
                return "file"

        _rejects(
            self,
            "pinned facts returned str",
            lambda: EffectSet((), Broken()).initial(_A),
        )
        _rejects(
            self, "repeated effect group id", lambda: EffectSet((_group(), _group()))
        )
        _rejects(self, "tuple of Domains", lambda: EffectSet((), None, _bad(("d",))))
        _rejects(self, "bad domain side", lambda: Domain("both", "path"))

    def test_writers_index_each_group_once_in_contract_order(self) -> None:
        move = Effect(_C, EffectKind.TRANSFER, None, _A, "w2")
        effects = EffectSet(
            (
                _group(id="g1", effects=(_produce(_A), _produce(_B))),
                _group(id="g2", action="w2", effects=(move,)),
                _group(id="g3", action="w3", effects=(_produce(_B),)),
            )
        )
        self.assertEqual(["g1", "g2"], [g.id for g in effects.writers([_A])])
        self.assertEqual(["g1", "g2", "g3"], [g.id for g in effects.writers([_B, _A])])
        self.assertEqual(["g2"], [g.id for g in effects.writers([_C])])
        self.assertEqual((), effects.writers([Cell("primary", "path", "z")]))

    def test_window_relations_are_closed(self) -> None:
        early, late = Window(1.0, 2.0), Window(2.0, 3.0)
        self.assertTrue(early.meets(late) and late.meets(early))
        self.assertFalse(early.before(late))
        self.assertTrue(early.before(Window(2.5, math.inf)))
        self.assertFalse(Window(2.5, math.inf).meets(early))
        self.assertTrue(Window(0.0, math.inf).contains(late))
        self.assertFalse(late.contains(early))


class ProofTest(unittest.TestCase):
    def test_proofs_validate_and_round_trip(self) -> None:
        proof = ProofCertificate(
            "p1", (_REF,), ("o1",), ("g1", "g2"), ("before(g1,o1)",)
        )
        self.assertEqual(proof, read_proof(json.loads(json.dumps(proof.document()))))
        _rejects(
            self,
            "names no obligation",
            lambda: ProofCertificate("p", (), (), (), ()),
        )
        _rejects(
            self,
            "groups must be sorted and unique",
            lambda: ProofCertificate("p", (_REF,), (), ("g2", "g1"), ()),
        )
        later = ObligationRef(_REF.rule_id, _REF.scope + "z")
        for obligations in ((_REF, _REF), (later, _REF)):
            with self.subTest(obligations=obligations):
                _rejects(
                    self,
                    "obligations must be sorted and unique",
                    lambda obligations=obligations: ProofCertificate(
                        "p", obligations, (), (), ()
                    ),
                )
        document = proof.document()
        _rejects(
            self, "proof must have exactly", lambda: read_proof({**document, "x": 1})
        )
        _rejects(
            self,
            "proof obligation is [rule, scope]",
            lambda: read_proof({**document, "obligations": [["a"]]}),
        )
        _rejects(
            self,
            "proof groups must be a list",
            lambda: read_proof({**document, "groups": "g1"}),
        )


_REF2 = ObligationRef("bytes.torn-read", "r1")


class CanonicalCellTest(unittest.TestCase):
    """Cells and domain prefixes take only canonical keys."""

    def test_only_canonical_keys_construct(self) -> None:
        for key in (ROOT, "a", "a/b", "a.b/c..d"):
            with self.subTest(key=key):
                self.assertTrue(is_canonical_key(key))
                self.assertEqual(key, Cell("primary", "rev", key).key)
        for key in ("./a//b", "a/", "/a", "a//b", ".", "..", "a/./b", "a/../b"):
            with self.subTest(key=key):
                self.assertFalse(is_canonical_key(key))
                _rejects(
                    self,
                    "is not canonical",
                    functools.partial(Cell, "primary", "path", key),
                )
        self.assertEqual("a/b", Cell("primary", "path", normalize_path("./a//b")).key)
        _rejects(self, "is not canonical", lambda: Domain("primary", "path", "d/"))

    def test_domain_membership_is_by_side_space_and_prefix(self) -> None:
        domain = Domain("primary", "path", "d")
        self.assertTrue(domain.contains(Cell("primary", "path", "d")))
        self.assertTrue(domain.contains(Cell("primary", "path", "d/e")))
        self.assertFalse(domain.contains(Cell("primary", "path", "de")))
        self.assertFalse(domain.contains(Cell("reference", "path", "d/e")))
        self.assertFalse(domain.contains(Cell("primary", "rev", "d/e")))
        self.assertTrue(Domain("primary", "path").contains(_A))


class GroupShapeTest(unittest.TestCase):
    """Groups write each cell once; guards and requirements stay on the group's side."""

    def test_a_group_writes_each_cell_at_most_once(self) -> None:
        _rejects(
            self,
            "writes one cell twice",
            lambda: _group(effects=(_produce(), _produce(value=ABSENT))),
        )
        transfer = Effect(_B, EffectKind.TRANSFER, None, _A, "mv")
        _rejects(
            self,
            "writes one cell twice",
            lambda: _group(effects=(transfer, _produce())),
        )
        self.assertEqual((_B, _A), transfer.writes())
        self.assertEqual(2, len(_group(effects=(transfer, _produce(_C))).effects))

    def test_guards_and_requirements_validate_and_encode(self) -> None:
        guard = Guard((_A,), Expect(((eq(_FILE),),)))
        effect = Effect(_A, EffectKind.PRODUCE, ABSENT, None, "rm", guard)
        self.assertEqual(
            {
                "cells": [["primary", "path", "a"]],
                "expect": [[eq(_FILE).document()]],
            },
            effect.document()["guard"],
        )
        requirement = Requirement((_B,), Expect(((of_class({"file"}),),)), _REF)
        group = _group(effects=(effect,), requires=(requirement,))
        self.assertEqual(
            [
                {
                    "cells": [["primary", "path", "b"]],
                    "expect": [[of_class({"file"}).document()]],
                    "obligation": ["bytes.stale-read", "r1"],
                }
            ],
            group.document()["requires"],
        )
        reference = Cell("reference", "path", "a")
        other_side = Guard((reference,), Expect(((ANY,),)))
        _rejects(
            self,
            "guard is on the effect's side",
            lambda: Effect(_A, EffectKind.PRODUCE, ABSENT, None, "rm", other_side),
        )
        _rejects(
            self,
            "requirement is on another side",
            lambda: _group(
                requires=(Requirement((reference,), Expect(((ANY,),)), _REF),)
            ),
        )
        _rejects(
            self,
            "guard spans sides",
            lambda: Guard((_A, reference), Expect(((ANY, ANY),))),
        )
        _rejects(
            self, "guard repeats a cell", lambda: Guard((_A, _A), Expect(((ANY, ANY),)))
        )
        _rejects(
            self,
            "rows constrain 1 cells, not 2",
            lambda: Guard((_A, _B), Expect(((ANY,),))),
        )
        _rejects(
            self,
            "requirement obligation is not an ObligationRef",
            lambda: Requirement((_A,), Expect(((ANY,),)), _bad("ref")),
        )
        _rejects(
            self,
            "requires is a tuple of Requirements",
            lambda: _group(requires=[requirement]),
        )


class ObservationShapeTest(unittest.TestCase):
    """One observation is one node on one side with a set of obligations."""

    def test_observations_never_mix_sides(self) -> None:
        reference = Cell("reference", "path", "a")
        _rejects(
            self,
            "spans sides ['primary', 'reference']",
            lambda: _observation(cells=(_A, reference), expect=Expect(((ANY, ANY),))),
        )

    def test_obligations_are_a_sorted_set_outside_the_content(self) -> None:
        first = _observation(obligations=(_REF2, _REF, _REF2))
        self.assertEqual((_REF, _REF2), first.obligations)
        self.assertEqual(first, _observation(obligations=(_REF, _REF2)))
        self.assertEqual(_observation().content(), first.content())
        self.assertNotIn("obligations", first.content())
        self.assertNotEqual(
            _observation().content(), _observation(window=Window(3.0, 5.0)).content()
        )

    def test_domain_names_the_aggregate_its_cells_belong_to(self) -> None:
        domain = Domain("primary", "path")
        listed = _observation(domain=domain)
        self.assertEqual(["primary", "path", ""], listed.document()["domain"])
        self.assertIsNone(_observation().document()["domain"])
        _rejects(
            self,
            "every cell is in its domain",
            lambda: _observation(domain=Domain("primary", "path", "d")),
        )
        _rejects(
            self,
            "every cell is in its domain",
            lambda: _observation(domain=Domain("reference", "path")),
        )
        _rejects(self, "domain is not a Domain", lambda: _observation(domain=_bad("d")))


class ConstraintIdentityTest(unittest.TestCase):
    """Constraint IDs are injective and include the obligation."""

    def test_ids_distinguish_every_field(self) -> None:
        pairs = (
            (Before("a,b", "c", _REF), Before("a", "b,c", _REF)),
            (Before("g", "o", _REF), Before("g", "o", _REF2)),
            (Before("g", "o", _REF), Supersedes("g", "o", _REF)),
            (MustAppearIn("g", "o", _REF), VisibleAfterAck("g", "o", _REF)),
            (Agree("o1", "o2", "raw", _REF), Agree("o1", "o2", "sorted", _REF)),
            (Exclusive(("v1", "v2"), _REF), Exclusive(("v1", "v3"), _REF)),
        )
        for x, y in pairs:
            with self.subTest(x=x, y=y):
                self.assertNotEqual(constraint_id(x), constraint_id(y))
        self.assertEqual(
            constraint_id(Before("a", "b", _REF)), constraint_id(Before("a", "b", _REF))
        )
        self.assertRegex(
            constraint_id(Before("a", "b", _REF)), r"^sha256:[0-9a-f]{64}$"
        )

    def test_exclusive_validates_and_sorts(self) -> None:
        exclusive = Exclusive(("v2", "v1"), _REF)
        self.assertEqual(("v1", "v2"), exclusive.outcome_vars)
        self.assertEqual(exclusive, Exclusive(("v1", "v2"), _REF))
        self.assertEqual(
            {
                "kind": "exclusive",
                "outcome_vars": ["v1", "v2"],
                "obligation": ["bytes.stale-read", "r1"],
            },
            exclusive.document(),
        )
        self.assertEqual((), exclusive.nodes())
        _rejects(
            self, "at least two outcome variables", lambda: Exclusive(("v1",), _REF)
        )
        _rejects(
            self, "repeats an outcome variable", lambda: Exclusive(("v1", "v1"), _REF)
        )
        _rejects(self, "bad outcome variable", lambda: Exclusive(("v1", ""), _REF))
        _rejects(
            self, "not an ObligationRef", lambda: Exclusive(("v1", "v2"), _bad("x"))
        )


class UnknownInitialTest(unittest.TestCase):
    """`EffectSet.initial` returns a Value or raises UnknownInitial, never None."""

    def test_unknown_initial_values_raise(self) -> None:
        with self.assertRaises(UnknownInitial):
            EffectSet().initial(_A)
        asked: list[Cell] = []

        class Partial:
            def initial(self, cell: Cell) -> Value:
                asked.append(cell)
                if cell == _B:
                    raise UnknownInitial("not in the fixture")
                return _FILE

        effects = EffectSet((), Partial())
        self.assertEqual(_FILE, effects.initial(_A))
        for _ in range(2):
            with self.assertRaises(UnknownInitial):
                effects.initial(_B)
        self.assertEqual([_A, _B], asked)

        class Silent:
            def initial(self, cell: Cell) -> Any:
                return None

        _rejects(
            self,
            "pinned facts returned NoneType",
            lambda: EffectSet((), Silent()).initial(_A),
        )

    def test_the_cache_is_not_a_constructor_input(self) -> None:
        with self.assertRaises(TypeError):
            cast(Any, EffectSet)(_initial={})
        _rejects(
            self,
            "repeated contract node id",
            lambda: EffectSet((_group(),), observations=(_observation(id="g1"),)),
        )
        _rejects(
            self,
            "tuple of Observations",
            lambda: EffectSet(observations=_bad([_observation()])),
        )
        derived = EffectSet(
            (_group(requires=(Requirement((_B,), Expect(((ANY,),)), _REF),)),),
            observations=(_observation(id="o1", obligations=(_REF, _REF2)),),
        )
        self.assertEqual(2, derived.supports(_REF))
        self.assertEqual(1, derived.supports(_REF2))
        self.assertEqual(0, derived.supports(ObligationRef("bytes.stale-read", "r9")))
        self.assertEqual(frozenset({_REF, _REF2}), derived.contract_obligations())
