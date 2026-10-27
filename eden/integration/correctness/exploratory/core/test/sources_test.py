# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import annotations

import importlib
import importlib.machinery
import importlib.util
import os
import py_compile
import random
import sys
import tempfile
import unittest
from collections.abc import Iterator, Mapping
from contextlib import contextmanager
from unittest import mock

from eden.integration.correctness.exploratory.core.sources import (
    CheckerSourceUnavailable,
    import_statements,
    ImportStatement,
    ModuleSource,
    read_module_source,
    source_closure,
    SourcePolicy,
    SourceReader,
)


_POLICY = SourcePolicy(follow=("pkg",), external=("env",))


def _reader(
    texts: Mapping[str, str], packages: frozenset[str] = frozenset()
) -> SourceReader:
    def read(name: str) -> ModuleSource | None:
        if name not in texts:
            return None
        return ModuleSource(texts[name].encode(), name in packages)

    return read


class ImportResolutionTest(unittest.TestCase):
    def test_relative_and_absolute_imports_resolve_to_absolute_names(self) -> None:
        source = ModuleSource(
            b"import a.b.c as alias\n"
            b"from . import sibling\n"
            b"from .. import uncle\n"
            b"from ..x.y import name as other\n"
            b"from .... import past_the_top\n"
            b"from q import *\n"
            b"def f():\n"
            b"    from .lazy import value\n",
            is_package=False,
        )
        self.assertEqual(
            (
                ImportStatement(1, "a.b.c"),
                ImportStatement(2, "pkg.sub", ("sibling",)),
                ImportStatement(3, "pkg", ("uncle",)),
                ImportStatement(4, "pkg.x.y", ("name",)),
                ImportStatement(5, None, ("past_the_top",)),
                ImportStatement(6, "q", (), star=True),
                ImportStatement(8, "pkg.sub.lazy", ("value",)),
            ),
            import_statements("pkg.sub.mod", source),
        )

    def test_a_package_resolves_relative_imports_inside_itself(self) -> None:
        self.assertEqual(
            (ImportStatement(1, "pkg.sub", ("child",)),),
            import_statements(
                "pkg.sub", ModuleSource(b"from . import child\n", is_package=True)
            ),
        )

    def test_unparseable_source_fails_closed(self) -> None:
        with self.assertRaises(CheckerSourceUnavailable):
            import_statements("pkg.bad", ModuleSource(b"def (:\n", is_package=False))


class SemanticDigestTest(unittest.TestCase):
    """A comment or docstring edit moved the semantic digest, so the
    next six campaigns could not activate any task (calibration fingerprint)."""

    _CODE = 'def f(x):\n    """Return x."""\n    return x\n'

    def _digest(self, text: str) -> str:
        return source_closure(
            ["pkg.mod"], policy=_POLICY, reader=_reader({"pkg": "", "pkg.mod": text})
        )["pkg.mod"]

    def test_comments_docstrings_and_formatting_keep_the_digest(self) -> None:
        base = self._digest(self._CODE)
        for edited in (
            self._CODE + "# a comment\n",
            '"""A module docstring."""\n' + self._CODE.replace("Return x.", "Echo."),
            "def f( x ):\n    return x   # same code\n",
        ):
            with self.subTest(edited=edited):
                self.assertEqual(base, self._digest(edited))

    def test_code_changes_move_the_digest(self) -> None:
        base = self._digest(self._CODE)
        for edited in (
            self._CODE.replace("return x", "return -x"),
            self._CODE + "VALUE = 'a string statement, not a docstring'\n",
            'def f(x):\n    """Return x."""\n    "a second string"\n    return x\n',
        ):
            with self.subTest(edited=edited):
                self.assertNotEqual(base, self._digest(edited))


_NS = "eden.integration.correctness"
_ENCLOSING = frozenset({"eden", "eden.integration"})


class EnclosingPackageTest(unittest.TestCase):
    """Imports through the packages enclosing the followed one cannot escape."""

    def _closure(self, root_text: str, **extra: str) -> Mapping[str, str]:
        texts = {
            "eden": "",
            "eden.integration": "",
            f"{_NS}.exploratory.t": root_text,
            f"{_NS}.exploratory.ok": "VALUE = 1\n",
            "eden.integration.helper": "BEHAVIOR = 1\n",
            "eden.integration.other_helper": "BEHAVIOR = 2\n",
            "eden.fs.client": "CLIENT = 1\n",
            **extra,
        }
        return source_closure(
            [f"{_NS}.exploratory.t"],
            policy=SourcePolicy(external=("eden.fs",)),
            reader=_reader(texts, _ENCLOSING),
        )

    def test_enclosing_package_imports_fail_closed(self) -> None:
        """Imports of an enclosing package, in either statement form, fail closed."""
        cases = {
            "from enclosing import helper": (
                "from eden.integration import helper\n",
                "imports 'eden.integration.helper' from 'eden.integration'",
            ),
            "relative climb into enclosing": (
                "from ... import other_helper\n",
                "imports 'eden.integration.other_helper'",
            ),
            "relative climb to top package": (
                "from .... import other_helper\n",
                "imports 'eden.other_helper' from 'eden'",
            ),
            "aliased enclosing package": (
                "import eden.integration as ei\n",
                "imports the enclosing package 'eden.integration'",
            ),
            "all three forms in one module": (
                "from eden.integration import helper\n"
                "from .... import other_helper\n"
                "import eden.integration as ei\n",
                "line 1: imports 'eden.integration.helper'",
            ),
            "direct form": (
                "from eden.integration.helper import BEHAVIOR\n",
                "imports 'eden.integration.helper', which is neither",
            ),
        }
        for name, (text, message) in cases.items():
            with self.subTest(case=name):
                with self.assertRaisesRegex(CheckerSourceUnavailable, message):
                    self._closure(text)

    def test_out_of_contract_imports_fail_closed(self) -> None:
        cases = {
            "relative import above the top": (
                "from ...... import anything\n",
                "climbs above the top level",
            ),
            "star from enclosing": (
                "from eden.integration import *\n",
                "imports the enclosing package",
            ),
            "plain enclosing import": ("import eden\n", "enclosing package 'eden'"),
            "enclosing child package": (
                "from eden import integration\n",
                "imports 'eden.integration' from 'eden'",
            ),
            "star from a followed package": (
                f"from {_NS} import *\n",
                f"star-imports package '{_NS}'",
            ),
            "followed name without source via enclosing": (
                "from eden.integration import correctness\n",
                f"imports '{_NS}', which has no source",
            ),
        }
        for name, (text, message) in cases.items():
            with self.subTest(case=name):
                with self.assertRaisesRegex(CheckerSourceUnavailable, message):
                    self._closure(text)

    def test_enclosing_packages_with_code_fail_closed(self) -> None:
        with self.assertRaisesRegex(CheckerSourceUnavailable, "'eden' encloses"):
            self._closure("from . import ok\n", eden="import os\n")

    def test_followed_and_external_members_of_enclosing_packages(self) -> None:
        closure = self._closure(
            "from eden.fs import client\n"
            "from . import ok\n"
            "from .ok import VALUE as renamed\n"
            "import eden.integration.correctness.exploratory.ok as alias\n"
            "from .ok import *\n"
        )
        self.assertEqual(
            [f"{_NS}.exploratory.ok", f"{_NS}.exploratory.t"], list(closure)
        )

    def test_empty_packages_are_not_digested(self) -> None:
        with_inits = self._closure(
            "from . import ok\n",
            **{_NS: "", f"{_NS}.exploratory": "\n"},
        )
        without = self._closure("from . import ok\n")
        self.assertEqual(without, with_inits)
        with_code = self._closure(
            "from . import ok\n", **{f"{_NS}.exploratory": "X = 1\n"}
        )
        self.assertIn(f"{_NS}.exploratory", with_code)


class SourceClosureTest(unittest.TestCase):
    _TEXTS: Mapping[str, str] = {
        "pkg.root": "from .helper import X\nimport env.client\nimport json\n",
        "pkg.helper": "from . import leaf\nfrom .leaf import attribute\n",
        "pkg.leaf": "attribute = 1\n",
        "pkg.unrelated": "from .root import X\n",
        "env.client": "import pkg.unrelated\n",
    }

    def _closure(self, texts: Mapping[str, str]) -> Mapping[str, str]:
        return source_closure(["pkg.root"], policy=_POLICY, reader=_reader(texts))

    def test_follows_imports_into_followed_packages_only(self) -> None:
        closure = self._closure(self._TEXTS)
        self.assertEqual(["pkg.helper", "pkg.leaf", "pkg.root"], list(closure))
        changed = dict(self._TEXTS, **{"pkg.leaf": "attribute = 2\n"})
        self.assertNotEqual(closure, self._closure(changed))
        for unrelated in ("pkg.unrelated", "env.client"):
            with self.subTest(unrelated=unrelated):
                untouched = dict(self._TEXTS, **{unrelated: "# changed\n"})
                self.assertEqual(closure, self._closure(untouched))

    def test_undeclared_imports_and_roots_fail_closed(self) -> None:
        cases = {
            "undeclared import": dict(
                self._TEXTS, **{"pkg.leaf": "import thirdparty.lib\n"}
            ),
            "unparsable dependency": dict(self._TEXTS, **{"pkg.leaf": "def (:\n"}),
        }
        for name, texts in cases.items():
            with self.subTest(case=name):
                with self.assertRaises(CheckerSourceUnavailable):
                    self._closure(texts)
        for root in ("env.client", "pkg.missing", "json"):
            with self.subTest(root=root):
                with self.assertRaises(CheckerSourceUnavailable):
                    source_closure(
                        [root],
                        policy=_POLICY,
                        reader=_reader(self._TEXTS),
                    )

    def test_external_packages_may_not_overlap_followed_ones(self) -> None:
        for external in ("pkg", "pkg.helper", ""):
            with self.subTest(external=external):
                policy = SourcePolicy(follow=("pkg",), external=(external or "p",))
                if not external:
                    policy = SourcePolicy(follow=("pkg.sub",), external=("pkg",))
                with self.assertRaisesRegex(CheckerSourceUnavailable, "overlaps"):
                    source_closure(
                        ["pkg.root"], policy=policy, reader=_reader(self._TEXTS)
                    )

    def test_unreadable_dependency_fails_closed(self) -> None:
        def read(name: str) -> ModuleSource | None:
            if name == "pkg.leaf":
                raise CheckerSourceUnavailable("unreadable")
            return _reader(self._TEXTS)(name)

        with self.assertRaises(CheckerSourceUnavailable):
            source_closure(["pkg.root"], policy=_POLICY, reader=read)


@contextmanager
def _checkout(files: Mapping[str, str]) -> Iterator[str]:
    """Write a package tree in a fresh directory and make it importable."""
    with tempfile.TemporaryDirectory() as root, mock.patch.dict(sys.modules):
        for path, text in files.items():
            os.makedirs(os.path.dirname(os.path.join(root, path)), exist_ok=True)
            with open(os.path.join(root, path), "w") as handle:
                handle.write(text)
        sys.path.insert(0, root)
        importlib.invalidate_caches()
        read_module_source.cache_clear()
        try:
            yield root
        finally:
            sys.path.remove(root)
            read_module_source.cache_clear()
            importlib.invalidate_caches()


class RealSourceTest(unittest.TestCase):
    _FILES: Mapping[str, str] = {
        "digestprobe/__init__.py": "",
        "digestprobe/prop.py": "from .helper import LIMIT\n",
        "digestprobe/helper.py": "LIMIT = 3\n",
    }
    _POLICY = SourcePolicy(follow=("digestprobe",))

    def _closure(self, files: Mapping[str, str]) -> tuple[str, Mapping[str, str]]:
        with _checkout(files) as root:
            return root, source_closure(["digestprobe.prop"], policy=self._POLICY)

    def test_digest_is_independent_of_the_checkout_root(self) -> None:
        first_root, first = self._closure(self._FILES)
        second_root, second = self._closure(self._FILES)
        self.assertNotEqual(first_root, second_root)
        self.assertEqual(first, second)
        self.assertEqual(["digestprobe.helper", "digestprobe.prop"], list(first))
        _root, changed = self._closure(
            dict(self._FILES, **{"digestprobe/helper.py": "LIMIT = 4\n"})
        )
        self.assertNotEqual(first, changed)

    def test_a_module_loaded_from_bytecode_is_read_from_its_source(self) -> None:
        """A packaged binary loads `.pyc` with the `.py` beside it; digest the `.py`."""
        with _checkout(self._FILES) as root:
            helper = os.path.join(root, "digestprobe", "helper.py")
            py_compile.compile(helper, cfile=helper + "c")
            name = "digestprobe.helper"
            loader = importlib.machinery.SourcelessFileLoader(name, helper + "c")
            spec = importlib.util.spec_from_loader(name, loader, origin=helper + "c")
            assert spec is not None
            spec.has_location = True
            module = importlib.util.module_from_spec(spec)
            with mock.patch.dict(sys.modules, {name: module}):
                source = read_module_source(name)
            assert source is not None
            self.assertEqual(b"LIMIT = 3\n", source.data)

    def test_a_module_without_python_source_fails_closed(self) -> None:
        with _checkout(self._FILES) as root:
            helper = os.path.join(root, "digestprobe", "helper.py")
            py_compile.compile(helper, cfile=helper + "c")
            os.remove(helper)
            importlib.invalidate_caches()
            with self.assertRaisesRegex(CheckerSourceUnavailable, "helper.py"):
                read_module_source("digestprobe.helper")
            with self.assertRaises(CheckerSourceUnavailable):
                source_closure(["digestprobe.prop"], policy=self._POLICY)
            self.assertIsNone(read_module_source("digestprobe.absent"))
            with self.assertRaises(CheckerSourceUnavailable):
                read_module_source("sys")
            with self.assertRaises(CheckerSourceUnavailable):
                source_closure(["digestprobe.absent"], policy=self._POLICY)


class DynamicLoadingTest(unittest.TestCase):
    """Loading code by a run-time name fails closed unless declared and literal."""

    _POLICY = SourcePolicy(follow=("pkg",), dynamic=("pkg.loaded",))

    def _closure(
        self, text: str, policy: SourcePolicy | None = None
    ) -> Mapping[str, str]:
        texts = {
            "pkg": "",
            "pkg.mod": text,
            "pkg.loaded": "VALUE = 1\n",
            "pkg.other": "VALUE = 2\n",
        }
        return source_closure(
            ["pkg.mod"],
            policy=policy or self._POLICY,
            reader=_reader(texts, frozenset({"pkg"})),
        )

    def test_a_declared_literal_load_is_followed_and_digested(self) -> None:
        for text in (
            "import importlib\nM = importlib.import_module('pkg.loaded')\n",
            "import importlib as il\nM = il.import_module('pkg.loaded')\n",
            "from importlib import import_module\nM = import_module('pkg.loaded')\n",
            "from importlib import import_module as im\nM = im('pkg.loaded')\n",
            "def f():\n    return __import__('pkg.loaded')\n",
        ):
            with self.subTest(text=text):
                closure = self._closure(text)
                self.assertEqual(["pkg.loaded", "pkg.mod"], list(closure))
        declared = self._closure("VALUE = 0\n")
        self.assertIn("pkg.loaded", declared)

    def test_every_other_dynamic_load_fails_closed(self) -> None:
        cases = {
            "undeclared literal": (
                "import importlib\nimportlib.import_module('pkg.other')\n",
                "does not declare",
            ),
            "non-literal": (
                "import importlib\nimportlib.import_module(NAME)\n",
                "non-literal",
            ),
            "f-string": (
                "import importlib\nimportlib.import_module(f'pkg.{x}')\n",
                "non-literal",
            ),
            "relative literal": (
                "import importlib\nimportlib.import_module('.loaded', 'pkg')\n",
                "exactly one literal",
            ),
            "relative single argument": (
                "from importlib import import_module\nimport_module('.loaded')\n",
                "relative module",
            ),
            "keyword package": (
                "import importlib\nimportlib.import_module('pkg.loaded', package='pkg')\n",
                "exactly one literal",
            ),
            "__import__ with fromlist": (
                "__import__('pkg.loaded', fromlist=['x'])\n",
                "exactly one literal",
            ),
            "aliased loader as value": (
                "from importlib import import_module as im\nload = im\n",
                "references 'im'",
            ),
            "importlib as value": (
                "import importlib\nload = importlib\n",
                "as a value",
            ),
            "getattr on importlib": (
                "import importlib\ngetattr(importlib, 'import_' + 'module')('pkg.other')\n",
                "as a value",
            ),
            "getattr by string": (
                "import os\ngetattr(os, 'import_module')\n",
                "names 'import_module' in a string",
            ),
            "builtins import": ("import builtins\n", "imports 'builtins'"),
            "builtins dict": (
                "__builtins__['__import__']('x')\n",
                "dynamic loading: (references '__builtins__'|names '__import__')",
            ),
            "__import__ as value": ("load = __import__\n", "references '__import__'"),
            "exec": ("exec('import pkg.other')\n", "references 'exec'"),
            "eval": ("eval('1')\n", "references 'eval'"),
            "compile": ("compile('x', 'f', 'exec')\n", "references 'compile'"),
            "reload": ("import importlib\nimportlib.reload(importlib)\n", "'reload'"),
            "module_from_spec": (
                "import importlib.util\nimportlib.util.module_from_spec(s)\n",
                "'module_from_spec'",
            ),
            "from util import loader": (
                "from importlib.util import spec_from_file_location\n",
                "'spec_from_file_location'",
            ),
            "exec_module": ("loader.exec_module(module)\n", "'exec_module'"),
            "machinery": ("import importlib.machinery\n", "'importlib.machinery'"),
            "runpy": ("import runpy\nrunpy.run_module('pkg.other')\n", "'runpy'"),
            "pickle": ("import pickle\n", "'pickle'"),
            "sys.modules": ("import sys\nsys.modules['pkg.other']\n", "sys.modules"),
            "sys as value": (
                "import sys\ngetattr(sys, 'mod' + 'ules')\n",
                "as a value",
            ),
            "from sys import modules": ("from sys import modules\n", "sys.modules"),
            "sys.__dict__": (
                "import sys\nsys.__dict__['modules']\n",
                "module attribute sys.__dict__",
            ),
            "sys.__getattribute__": (
                "import sys\nsys.__getattribute__('modules')\n",
                "module attribute sys.__getattribute__",
            ),
            "vars(sys)": ("import sys\nvars(sys)['modules']\n", "as a value"),
            "importlib.__dict__": (
                "import importlib\nimportlib.__dict__[NAME]\n",
                "module attribute importlib.__dict__",
            ),
            "function globals": ("f.__globals__\n", "'__globals__'"),
            "function-local loader": (
                "def f(n):\n    import importlib\n    return importlib.import_module(n)\n",
                "non-literal",
            ),
        }
        for name, (text, message) in cases.items():
            with self.subTest(case=name):
                with self.assertRaisesRegex(CheckerSourceUnavailable, message):
                    self._closure(text)

    def test_a_declared_module_must_be_followed(self) -> None:
        with self.assertRaisesRegex(CheckerSourceUnavailable, "outside the followed"):
            self._closure(
                "VALUE = 0\n", SourcePolicy(follow=("pkg",), dynamic=("env.x",))
            )

    def test_find_spec_and_sys_attributes_are_not_loads(self) -> None:
        closure = self._closure(
            "import importlib.util\nimport sys\n"
            "SPEC = importlib.util.find_spec(NAME)\nNAMES = sys.stdlib_module_names\n"
        )
        self.assertEqual(["pkg.loaded", "pkg.mod"], list(closure))

    def test_mutation_fuzz_catches_every_listed_escape(self) -> None:
        """Every listed escape spliced into every position of a clean module fails closed."""
        clean = [
            "import json",
            "VALUE = 1",
            "def check(x):",
            "    return json.dumps(x)",
            "class Probe:",
            "    limit = 3",
        ]
        escapes = [
            "import importlib; importlib.import_module(NAME)",
            "import importlib as il; il.import_module('pkg.other')",
            "from importlib import import_module as im; im(NAME)",
            "__import__(NAME)",
            "exec(CODE)",
            "eval(CODE)",
            "import runpy",
            "import sys; sys.modules.get(NAME)",
            "from importlib import util as u; u.module_from_spec(S)",
            "getattr(OBJ, '__import__')",
            "import importlib; L = [importlib]",
        ]
        rng = random.Random(918)
        trials = 0
        for escape in escapes:
            for position in range(len(clean) + 1):
                lines = list(clean)
                indent = (
                    "    "
                    if 0 < position < len(clean) and clean[position - 1].endswith(":")
                    else ""
                )
                lines.insert(position, indent + escape)
                with self.subTest(escape=escape, position=position):
                    with self.assertRaises(CheckerSourceUnavailable):
                        self._closure("\n".join(lines) + "\n")
                trials += 1
        for _ in range(200):
            body = rng.sample(clean[:2], 2)
            self._closure("\n".join(body) + "\n")
        self.assertEqual(len(escapes) * (len(clean) + 1), trials)
