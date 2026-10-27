# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import annotations

import ast
import json
import os
import re
import subprocess
import sys
import unittest
from pathlib import Path

from eden.integration.correctness.exploratory.core import (
    campaign,
    containment,
    differential,
    effects,
    exhaustive,
    facts,
    generate,
    history,
    identity,
    runspec,
    scheduler,
    semantics,
    solver,
)


FORBIDDEN_IMPORT = re.compile(
    r"^\s*(from|import)\s+"
    r"(eden\.integration\.correctness\.exploratory\.("
    + "eden"
    + r"fs|fa"
    + r"ke)|eden\.fs|thrift)"
)


class BoundaryTest(unittest.TestCase):
    def test_core_imports_no_target_code(self) -> None:
        core = Path(__file__).parent.parent
        modules = (
            campaign,
            runspec,
            scheduler,
            history,
            containment,
            differential,
            facts,
            effects,
            identity,
            semantics,
            solver,
            exhaustive,
        )
        required = tuple(Path(module.__file__).name for module in modules)
        self.assertEqual(
            {
                "campaign.py",
                "runspec.py",
                "scheduler.py",
                "history.py",
                "containment.py",
                "differential.py",
                "facts.py",
                "effects.py",
                "identity.py",
                "semantics.py",
                "solver.py",
                "exhaustive.py",
            },
            set(required),
        )
        missing = [name for name in required if not (core / name).is_file()]
        self.assertEqual([], missing)

        violations = [
            f"{path.name}:{line_number}: {line}"
            for path in core.glob("*.py")
            for line_number, line in enumerate(path.read_text().splitlines(), start=1)
            if FORBIDDEN_IMPORT.match(line)
        ]

        self.assertEqual([], violations)

    def test_core_contains_no_target_data(self) -> None:
        core = Path(__file__).parent.parent
        forbidden = (
            "eden" + "fs",
            "fa" + "ke",
            "CH" + "URN",
            "repo" + "sitory",
            "reg" + "ister",
        )
        # The RunSpec wire name is frozen: every recorded spec carries it.
        frozen = {"spec_types.py": '"edenfs-exploratory-run"'}
        violations = [
            f"{path.name}: {literal}"
            for path in core.glob("*.py")
            for literal in forbidden
            if literal in path.read_text().replace(frozen.get(path.name, "\0"), "")
        ]

        self.assertEqual([], violations)


PACKAGE = "eden.integration.correctness.exploratory"
# Generation may read static action declarations, footprints, and spec
# types; it must never reach checker, property, or contracts code.
GENERATION_MODULES = frozenset(
    f"{PACKAGE}.core.{name}"
    for name in (
        "differential",
        "effects",
        "facts",
        "generate",
        "graph_rules",
        "history",
        "jsonio",
        "property",
        "runspec",
        "sites",
        "spec_types",
        "target",
    )
)
# Module and builtin names that import or run code by name at runtime.
DYNAMIC_IMPORT_MODULES = frozenset(
    {"importlib", "imp", "pkgutil", "runpy", "zipimport"}
)
DYNAMIC_IMPORT_CALLS = frozenset({"__import__", "compile", "eval", "exec"})
_LOADED_PROGRAM = f"""
import json, sys
import {PACKAGE}.core.generate
print(json.dumps(sorted(n for n in sys.modules if n.startswith("{PACKAGE}."))))
"""


def _loaded_by_fresh_import() -> list[str]:
    """Import the generator in a new interpreter and list the package modules it loaded."""
    root = os.path.abspath(__file__)
    for _ in range(__name__.count(".") + 1):
        root = os.path.dirname(root)
    env = dict(os.environ, PYTHONPATH=os.pathsep.join([root, *sys.path]))
    lib = os.path.normpath(
        os.path.join(os.path.dirname(os.path.abspath(sys.executable)), "..", "lib")
    )
    if os.path.isdir(lib):
        env["LD_LIBRARY_PATH"] = os.pathsep.join(
            filter(None, [lib, env.get("LD_LIBRARY_PATH")])
        )
    output = subprocess.run(
        [sys.executable, "-c", _LOADED_PROGRAM],
        env=env,
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    return json.loads(output)


class GenerationBoundaryTest(unittest.TestCase):
    def test_generation_imports_only_declarations(self) -> None:
        source = Path(generate.__file__).read_text()
        local: set[str] = set()
        external: set[str] = set()
        for node in ast.walk(ast.parse(source)):
            if isinstance(node, ast.ImportFrom) and node.level:
                local.add(node.module or "")
            elif isinstance(node, ast.ImportFrom):
                external.add(node.module or "")
            elif isinstance(node, ast.Import):
                external.update(alias.name for alias in node.names)
        self.assertEqual({"effects", "runspec", "sites", "target"}, local)
        self.assertEqual(
            set(),
            {
                name
                for name in external
                if name.split(".")[0] not in sys.stdlib_module_names
                and name != "__future__"
            },
        )

    def test_generation_has_no_dynamic_imports(self) -> None:
        """An import by name, anywhere in the module, would bypass the AST check."""
        tree = ast.parse(Path(generate.__file__).read_text())
        modules = {
            alias.name.split(".")[0]
            for node in ast.walk(tree)
            if isinstance(node, (ast.Import, ast.ImportFrom))
            for alias in (
                node.names
                if isinstance(node, ast.Import)
                else [ast.alias(node.module or "")]
            )
        }
        self.assertEqual(set(), modules & DYNAMIC_IMPORT_MODULES)
        calls = {
            node.func.id
            for node in ast.walk(tree)
            if isinstance(node, ast.Call) and isinstance(node.func, ast.Name)
        }
        self.assertEqual(set(), calls & DYNAMIC_IMPORT_CALLS)
        names = [
            node.value
            for node in ast.walk(tree)
            if isinstance(node, ast.Constant)
            and isinstance(node.value, str)
            and PACKAGE in node.value
        ]
        self.assertEqual([], names)

    def test_importing_generation_loads_no_checker_or_target_code(self) -> None:
        names = set(_loaded_by_fresh_import())
        # Drop the namespace packages that merely contain loaded modules.
        loaded = {
            name
            for name in names
            if not any(other.startswith(name + ".") for other in names)
        }
        self.assertIn(f"{PACKAGE}.core.generate", loaded)
        self.assertEqual(set(), loaded - GENERATION_MODULES)
        for forbidden in ("checker", "sources", "contracts", "properties", "oracles"):
            self.assertEqual([], [name for name in loaded if forbidden in name])
