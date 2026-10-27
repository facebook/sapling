# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The semantic digest binds a verdict to the code that judged it: the core
checker and every helper it imports, the target's sources, and its static
declarations and budgets. Any change to one of them moves the digest, and a
source that cannot be read fails closed instead of producing a digest.
"""

from __future__ import annotations

import functools
import json
import unittest
from collections.abc import Callable
from dataclasses import replace
from typing import Any, cast
from unittest import mock

from eden.integration.correctness.exploratory.core import checker as checker_module
from eden.integration.correctness.exploratory.core.checker import (
    bind,
    check,
    checker_sources,
    CheckerSourceUnavailable,
    semantic_digest,
)
from eden.integration.correctness.exploratory.core.facts import (
    ErrnoUse,
    FieldKind,
    FieldSpec,
    ResultSchema,
)
from eden.integration.correctness.exploratory.core.sources import (
    ModuleSource,
    read_module_source,
)
from eden.integration.correctness.exploratory.core.target import (
    Binding,
    Grouping,
    Role,
    Target,
    TargetSources,
)
from eden.integration.correctness.exploratory.core.test import checker_testing as t


_NS = "eden.integration.correctness"


_CORE = f"{_NS}.exploratory.core"


_SYNTHETIC = f"{_NS}.exploratory.synthetic"


_DIFFERENTIAL = f"{_CORE}.differential"


_ENCODING = f"{_NS}.exploratory.edenfs.encoding"


_REPO = f"{_NS}.repo"


_PROPERTY = f"{_SYNTHETIC}.properties"


_PREDICATE = f"{_SYNTHETIC}.predicates"


_IMPLEMENTATION = f"{_SYNTHETIC}.target"


_IMPLEMENTATION_HELPER = f"{_SYNTHETIC}.vocabulary"


_DECLARED = f"{_SYNTHETIC}.loaded_by_name"


_LOCAL_IMPORT = f"{_SYNTHETIC}.lazy"


_DIAGNOSTICS = f"{_SYNTHETIC}.diagnostics"


_OUTSIDE = "outside_package.helper"


_SYNTHETIC_SOURCES = {
    _PROPERTY: (
        "from ..core.differential import compare\n"
        "from ..edenfs import encoding\n"
        "from ... import repo\n"
        "import outside_package.helper\n"
        "\n"
        "def collect(check_input, sink):\n"
        "    from . import lazy\n"
    ),
    _DIFFERENTIAL: "from .history import Event\n\ndef compare(a, b):\n    return a == b\n",
    _ENCODING: "WIDTH = 8\n",
    _REPO: "THRESHOLD = 0\n",
    _PREDICATE: "def capable(spec, facts, action):\n    return False\n",
    _IMPLEMENTATION: (
        "from .vocabulary import KINDS\nfrom ..core.actions import CORE_ACTION_DEFS\n"
    ),
    _IMPLEMENTATION_HELPER: "KINDS = ()\n",
    _DECLARED: "TABLE = {}\n",
    _LOCAL_IMPORT: "VALUE = 1\n",
    _DIAGNOSTICS: "from .properties import collect\n",
    _OUTSIDE: "THRESHOLD = 0\n",
}


def _defined_in(module: str, function: Callable[..., Any]) -> Any:
    """Return a copy of a function that names `module` as its defining module."""

    def defined(*args: Any) -> Any:
        return function(*args)

    defined.__module__ = module
    return defined


class _SourceGraph:
    """A source reader over the real core modules plus synthetic modules.

    Synthetic modules stand for target code, helpers, and fixtures that the
    digest must reach only through their imports.
    """

    def __init__(
        self, texts: dict[str, str] | None = None, unreadable: str | None = None
    ) -> None:
        self._texts = dict(_SYNTHETIC_SOURCES if texts is None else texts)
        self._unreadable = unreadable

    def __call__(self, name: str) -> ModuleSource | None:
        if name == self._unreadable:
            raise CheckerSourceUnavailable(f"{name} is unreadable")
        if name in self._texts:
            return ModuleSource(self._texts[name].encode(), is_package=False)
        if name.startswith(_CORE + "."):
            return read_module_source(name)
        return None

    def text(self, name: str) -> str:
        source = self(name)
        assert source is not None
        return source.data.decode()

    def mutated(self, name: str, text: str) -> _SourceGraph:
        return _SourceGraph({**self._texts, name: text}, self._unreadable)

    def unreadable(self, name: str) -> _SourceGraph:
        return _SourceGraph(self._texts, name)

    def target(self) -> Target:
        quiet = t.quiet()
        return t.target(
            replace(
                quiet,
                collect=_defined_in(_PROPERTY, quiet.collect),
            ),
            timeout_capable=_defined_in(_PREDICATE, t.never),
            sources=TargetSources(
                implementation=_IMPLEMENTATION,
                modules=(_DECLARED,),
                external=("outside_package",),
            ),
        )


class SemanticDigestTest(unittest.TestCase):
    _SPEC = t.spec([t.action("r", "read")])

    def test_semantic_digest_binds_core_and_target_sources(self) -> None:
        core_only = semantic_digest()
        self.assertEqual(core_only, semantic_digest())
        local = t.target(t.quiet())
        self.assertNotEqual(core_only, semantic_digest(local))
        self.assertEqual(
            semantic_digest(local),
            t.report(check(local, self._SPEC, t.READ_OK)).bindings.semantic,
        )
        self.assertEqual(core_only, bind(self._SPEC, t.READ_OK, None).semantic)
        bare_partial = t.target(
            replace(t.quiet(), collect=cast(Any, functools.partial(t.quiet().collect)))
        )
        self.assertEqual(semantic_digest(local), semantic_digest(bare_partial))
        stdlib = replace(t.quiet(), collect=cast(Any, json.dumps))
        declared = t.target(
            stdlib, sources=TargetSources(implementation=__name__, packages=("json",))
        )
        self.assertIn("json.decoder", checker_sources(declared))
        self.assertNotEqual(semantic_digest(local), semantic_digest(declared))
        more_external = t.target(
            t.quiet(),
            sources=TargetSources(implementation=__name__, external=("unused",)),
        )
        self.assertNotEqual(semantic_digest(local), semantic_digest(more_external))

    def test_semantic_digest_binds_static_declarations_and_budgets(self) -> None:
        # A diagnostic errno keeps the target valid whether `read` is modeled.
        diagnostic = t.action_def(
            "read", "input", modeled=True, result=t.UNMODELED_INPUT_RESULT
        )
        local = t.target(t.quiet(), actions={"read": diagnostic})
        digest = semantic_digest(local)
        read = local.actions["read"]
        changes = {
            "result schema": replace(
                read,
                result=ResultSchema(
                    fail=(FieldSpec("errno", FieldKind.INT, required=False),),
                    errnos={"ENOENT": ErrnoUse.STATE},
                ),
            ),
            "roles": replace(
                read,
                roles=frozenset({Role.PRODUCE}),
                binding=Binding.PATH,
                grouping=Grouping.SINGLE,
            ),
            "binding": replace(read, binding=Binding.PATH),
            "modeled": replace(read, modeled=False),
        }
        for name, changed in changes.items():
            with self.subTest(change=name):
                target = replace(local, actions={"read": changed})
                self.assertNotEqual(digest, semantic_digest(target))
        budgets = cast(dict[str, int], checker_module.SOLVER_BUDGETS)
        with mock.patch.dict(budgets, {"decision_work": 1}):
            self.assertNotEqual(digest, semantic_digest(local))
        self.assertEqual(digest, semantic_digest(local))

    def test_core_sources_cover_the_checker_and_its_helpers(self) -> None:
        names = set(checker_sources())
        for module in (
            "actions",
            "checker",
            "effects",
            "facts",
            "graph_rules",
            "history",
            "identity",
            "runspec",
            "sites",
            "sources",
            "spec_types",
            "target",
        ):
            self.assertIn(f"{_CORE}.{module}", names)
        self.assertNotIn(f"{_CORE}.test.checker_testing", names)
        self.assertIn(
            f"{_CORE}.test.checker_testing", checker_sources(t.target(t.quiet()))
        )

    def test_semantic_digest_fails_closed_without_source(self) -> None:
        """A replay digest never silently weakens: unreadable source refuses."""
        anonymous = cast(Any, lambda *_: None)
        anonymous.__module__ = None
        targets = {
            name: t.target(replace(t.quiet(), collect=collect))
            for name, collect in (
                ("builtin", cast(Any, max)),
                ("partial over a builtin", functools.partial(cast(Any, max))),
                ("no module", anonymous),
            )
        }
        targets["partial binding arguments"] = t.target(
            replace(
                t.quiet(),
                collect=cast(
                    Any, functools.partial(cast(Any, t.quiet().collect), None)
                ),
            )
        )
        targets["undeclared standard-library package"] = t.target(
            replace(t.quiet(), collect=cast(Any, json.dumps))
        )
        targets["external inside the checker namespace"] = t.target(
            t.quiet(),
            sources=TargetSources(implementation=__name__, external=(f"{_NS}.lib",)),
        )
        targets["no declaration"] = t.target(t.quiet(), sources=None)
        targets["missing implementation"] = t.target(
            t.quiet(), sources=TargetSources(implementation=f"{_NS}.no_such_module")
        )
        targets["builtin implementation"] = t.target(
            t.quiet(), sources=TargetSources(implementation="sys")
        )
        targets["missing declared module"] = t.target(
            t.quiet(),
            sources=TargetSources(
                implementation=__name__, modules=(f"{_NS}.no_such_module",)
            ),
        )
        targets["builtin footprint"] = t.target(
            t.quiet(),
            actions={
                "read": replace(t.action_def("read", "input"), footprint=cast(Any, max))
            },
        )
        for name, target in targets.items():
            with self.subTest(source=name):
                with self.assertRaises(CheckerSourceUnavailable):
                    semantic_digest(target)
                with self.assertRaises(CheckerSourceUnavailable):
                    check(target, self._SPEC, t.READ_OK)

    def test_semantic_digest_fails_closed_on_an_unreadable_dependency(self) -> None:
        graph = _SourceGraph()
        target = graph.target()
        self.assertIsInstance(semantic_digest(target, reader=graph), str)
        for name in (_ENCODING, _REPO, f"{_CORE}.actions", _DECLARED):
            with self.subTest(module=name):
                with self.assertRaises(CheckerSourceUnavailable):
                    semantic_digest(target, reader=graph.unreadable(name))
                with self.assertRaises(CheckerSourceUnavailable):
                    semantic_digest(
                        target, reader=graph.mutated(name, "def broken(:\n")
                    )

    def test_semantic_digest_changes_with_every_dependency(self) -> None:
        """A change to any module that can change a verdict changes the digest.

        Covers four modules (the core action table, core
        differential helpers, EdenFS register encoding, and the seeded
        repository fixture), each reached only through an import.
        """
        graph = _SourceGraph()
        target = graph.target()
        original = semantic_digest(target, reader=graph)
        covered = set(checker_sources(target, reader=graph))
        dependencies = {
            "core action table": f"{_CORE}.actions",
            "core history": f"{_CORE}.history",
            "core checker": f"{_CORE}.checker",
            "core facts": f"{_CORE}.facts",
            "core effects": f"{_CORE}.effects",
            "core differential helpers": _DIFFERENTIAL,
            "EdenFS register encoding": _ENCODING,
            "seeded repository fixture": _REPO,
            "property module": _PROPERTY,
            "predicate module": _PREDICATE,
            "target implementation": _IMPLEMENTATION,
            "implementation helper": _IMPLEMENTATION_HELPER,
            "declared behavior module": _DECLARED,
            "function-local import": _LOCAL_IMPORT,
        }
        for name, module in dependencies.items():
            with self.subTest(dependency=name):
                self.assertIn(module, covered)
                changed = graph.mutated(module, graph.text(module) + "\nX = 1\n")
                self.assertNotEqual(original, semantic_digest(target, reader=changed))
        for name, module in {
            "unimported diagnostics": _DIAGNOSTICS,
            "module outside the namespace": _OUTSIDE,
        }.items():
            with self.subTest(unrelated=name):
                self.assertNotIn(module, covered)
                changed = graph.mutated(module, "THRESHOLD = 99\n")
                self.assertEqual(original, semantic_digest(target, reader=changed))
        self.assertNotIn(f"{_DIFFERENTIAL}.compare", covered)
        self.assertEqual(original, semantic_digest(target, reader=_SourceGraph()))

    def test_a_helper_threshold_change_flips_the_digest(self) -> None:
        """A helper constant that flips a verdict flips the digest.

        A property module imports a helper outside every followed package;
        the digest refuses until both are declared, then binds the helper.
        """
        graph = _SourceGraph(
            {
                "prop_mod": "from helper_mod import THRESHOLD\n",
                "helper_mod": "THRESHOLD = 0\n",
            }
        )
        quiet = t.quiet()
        prop = replace(quiet, collect=_defined_in("prop_mod", quiet.collect))
        for packages in ((), ("prop_mod",)):
            with self.subTest(packages=packages):
                undeclared = t.target(
                    prop,
                    sources=TargetSources(implementation=__name__, packages=packages),
                )
                with self.assertRaises(CheckerSourceUnavailable):
                    semantic_digest(undeclared, reader=graph)
        target = t.target(
            prop,
            sources=TargetSources(
                implementation=__name__, packages=("helper_mod", "prop_mod")
            ),
        )
        before = semantic_digest(target, reader=graph)
        flipped = graph.mutated("helper_mod", "THRESHOLD = 1\n")
        self.assertNotEqual(before, semantic_digest(target, reader=flipped))
        same = graph.mutated("helper_mod", "THRESHOLD = 0\n")
        self.assertEqual(before, semantic_digest(target, reader=same))
