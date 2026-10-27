# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The source modules a verdict depends on, found by following static imports.

A checker digest covers the docstring-free syntax tree of every module
that can change a verdict. The modules are found from explicit roots (the checker's core
modules, each property callable's module, and the target's declared
`TargetSources`) by following every static ``import`` statement, at any
depth in the module, into the followed packages. Every other import must
name the standard library or a declared external package, whose identity
the build attests rather than this digest. A module is named by its
absolute dotted name and digested by its syntax tree without docstrings
(`semantic_text`), so the result is the same in every checkout. An
undeclared import, or a followed module that exists but has no readable
Python source, fails closed.

No followed module other than this one (which names the loaders it
forbids) may load code by a name chosen at run time: every
``__import__``, ``importlib.import_module``, alias of either, loader,
``exec``/``eval``/``compile``, import-machinery module, or reference to
``sys.modules`` fails closed, except a direct call that names a literal
absolute module the policy declares in `dynamic`; declared modules are
roots of the closure, so their source is digested too. This scan is a
best-effort guard over the syntax it lists, not a proof: code that
reaches a loader some other way (an attribute of an object it was
handed, for example) is not detected.
"""

from __future__ import annotations

import ast
import functools
import importlib.util
import sys
from collections.abc import Callable, Iterable, Mapping, Sequence
from dataclasses import dataclass
from typing import cast, NoReturn

from .jsonio import sha256 as _sha256


SOURCE_NAMESPACE = "eden.integration.correctness"


class CheckerSourceUnavailable(RuntimeError):
    """A module that can change a verdict has no readable source to digest."""


@dataclass(frozen=True)
class ModuleSource:
    """One module's source bytes and whether it is a package."""

    data: bytes
    is_package: bool


SourceReader = Callable[[str], ModuleSource | None]
"""Return a module's source, None when no module with code has that name.

Raises CheckerSourceUnavailable when the module exists but its Python
source cannot be read.
"""


@functools.cache
def read_module_source(name: str) -> ModuleSource | None:
    """Read one module's source through its loader, without importing the module.

    A module loaded from bytecode is read from the ``.py`` source packaged
    beside it, as a packaged binary ships both. Returns None for a name
    that resolves to no module, or to a namespace package, which has no
    code. Results are cached for the process: the
    digest describes the code this process loaded.
    """
    try:
        spec = importlib.util.find_spec(name)
    except ImportError:
        return None
    except ValueError as error:
        raise CheckerSourceUnavailable(
            f"module {name!r} has no spec: {error}"
        ) from error
    if spec is None:
        return None
    if spec.origin is None and spec.submodule_search_locations is not None:
        return None
    get_data = getattr(spec.loader, "get_data", None)
    origin = str(spec.origin)
    path = origin[:-1] if origin.endswith(".pyc") else origin
    if not spec.has_location or get_data is None or not path.endswith(".py"):
        raise CheckerSourceUnavailable(
            f"module {name!r} has no readable Python source (origin {origin!r})"
        )
    try:
        data = get_data(path)
    except OSError as error:
        raise CheckerSourceUnavailable(
            f"module {name!r} source is unreadable: {error}"
        ) from error
    return ModuleSource(bytes(data), spec.submodule_search_locations is not None)


def _ancestors(name: str) -> list[str]:
    parts = name.split(".")
    return [".".join(parts[:index]) for index in range(1, len(parts))]


@dataclass(frozen=True)
class ImportStatement:
    """One import statement, resolved to absolute module names.

    `module` is the module ``import`` names or ``from`` imports from, None
    when a relative import climbs above the top-level package. `names` are
    the names a ``from`` statement binds (empty for ``import``), and `star`
    marks ``from module import *``.
    """

    line: int
    module: str | None
    names: tuple[str, ...] = ()
    star: bool = False


def _resolve_from(name: str, is_package: bool, node: ast.ImportFrom) -> str | None:
    """Return the absolute module a ``from ... import`` names, or None past the top."""
    if node.level == 0:
        return node.module
    parts = name.split(".") if is_package else name.split(".")[:-1]
    if node.level - 1 > len(parts) - 1:
        return None
    base = parts[: len(parts) - (node.level - 1)]
    return ".".join([*base, node.module] if node.module else base)


@functools.lru_cache(maxsize=1024)
def _nodes(name: str, source: ModuleSource) -> tuple[ast.AST, ...]:
    """Parse a module once and return every node in walk order.

    Raises CheckerSourceUnavailable when the source does not parse.
    """
    try:
        tree = ast.parse(source.data, filename=name)
    except (SyntaxError, ValueError) as error:
        raise CheckerSourceUnavailable(
            f"module {name!r} source does not parse: {error}"
        ) from error
    return tuple(ast.walk(tree))


_DOCUMENTED = (ast.Module, ast.ClassDef, ast.FunctionDef, ast.AsyncFunctionDef)


@functools.lru_cache(maxsize=1024)
def semantic_text(name: str, source: ModuleSource) -> bytes:
    """Return a module's syntax tree without docstrings, the text its digest binds.

    Comments, formatting, and docstrings never change a verdict, so they
    never change the semantic digest (and with it a campaign's calibration
    fingerprint). Raises CheckerSourceUnavailable when the source does not
    parse.
    """
    _nodes(name, source)
    tree = ast.parse(source.data, filename=name)
    for node in ast.walk(tree):
        if not isinstance(node, _DOCUMENTED) or not node.body:
            continue
        first = node.body[0]
        if (
            isinstance(first, ast.Expr)
            and isinstance(first.value, ast.Constant)
            and isinstance(first.value.value, str)
        ):
            node.body = node.body[1:] or [ast.Pass()]
    return ast.dump(tree).encode()


@functools.lru_cache(maxsize=1024)
def import_statements(name: str, source: ModuleSource) -> tuple[ImportStatement, ...]:
    """Return every import statement in a module, resolved to absolute names.

    Every statement counts, at any depth, including function-local and
    ``TYPE_CHECKING`` ones. Raises CheckerSourceUnavailable when the source
    does not parse.
    """
    statements: list[ImportStatement] = []
    for node in _nodes(name, source):
        if isinstance(node, ast.Import):
            statements.extend(
                ImportStatement(node.lineno, alias.name) for alias in node.names
            )
        elif isinstance(node, ast.ImportFrom):
            names = tuple(alias.name for alias in node.names if alias.name != "*")
            statements.append(
                ImportStatement(
                    node.lineno,
                    _resolve_from(name, source.is_package, node),
                    names,
                    star=len(names) < len(node.names),
                )
            )
    return tuple(sorted(statements, key=lambda s: (s.line, s.module or "", s.names)))


def _within(name: str, packages: Iterable[str]) -> bool:
    return any(
        name == package or name.startswith(package + ".") for package in packages
    )


def _is_stdlib(name: str) -> bool:
    return name.partition(".")[0] in sys.stdlib_module_names


@dataclass(frozen=True)
class SourcePolicy:
    """Which imports a source closure follows, and which it may leave behind.

    Imports into `follow` are digested. Imports of the standard library
    and of `external` packages are the runtime environment: the checker
    digest does not cover them, and the build attests them. Any other
    import is an undeclared dependency and fails closed. `dynamic` names
    the followed modules a followed module may load with a direct call
    that names them literally; each is a root of the closure.
    """

    follow: tuple[str, ...] = (SOURCE_NAMESPACE,)
    external: tuple[str, ...] = ()
    dynamic: tuple[str, ...] = ()


CHECKER_POLICY = SourcePolicy()

_FOLLOW = "followed"
_ENVIRONMENT = "environment"
_PARENT = "parent"
_UNDECLARED = "undeclared"


def _codeless(source: ModuleSource | None) -> bool:
    return source is None or not source.data.strip()


# The module that implements these checks names the loaders it forbids;
# it is digested like every other followed module.
_CHECKER_MODULE = __name__
_LOADER_FUNCTIONS = frozenset({"import_module", "__import__"})
_FORBIDDEN_NAMES = frozenset(
    {"__import__", "exec", "eval", "compile", "__builtins__", "__loader__"}
)
_FORBIDDEN_ATTRIBUTES = frozenset(
    {
        "__import__",
        "import_module",
        "reload",
        "module_from_spec",
        "spec_from_file_location",
        "spec_from_loader",
        "exec_module",
        "load_module",
        "run_module",
        "run_path",
        "LazyLoader",
        "SourceFileLoader",
        "SourcelessFileLoader",
        "ExtensionFileLoader",
        "zipimporter",
        "meta_path",
        "path_hooks",
        "path_importer_cache",
        "__globals__",
        "__builtins__",
        "__loader__",
        "__subclasses__",
        "f_globals",
        "f_builtins",
    }
)
_FORBIDDEN_STRINGS = _FORBIDDEN_ATTRIBUTES | {"importlib", "builtins", "runpy"}
_FORBIDDEN_MODULES = (
    "builtins",
    "code",
    "codeop",
    "imp",
    "importlib.abc",
    "importlib.machinery",
    "importlib.metadata",
    "importlib.resources",
    "importlib._bootstrap",
    "importlib._bootstrap_external",
    "marshal",
    "pickle",
    "pkgutil",
    "runpy",
    "shelve",
    "zipimport",
)
# The importlib names a module may bind and how each may be used.
_IMPORTLIB = "importlib"
_SYS = "sys"


@dataclass(frozen=True)
class DynamicLoad:
    """One dynamic-loading construct in a module: a permitted literal load or a problem.

    `module` is the literal absolute module a permitted direct call names;
    `problem` says why anything else fails closed.
    """

    line: int
    module: str | None = None
    problem: str | None = None


def _bindings(  # noqa: C901
    nodes: Sequence[ast.AST],
) -> tuple[set[str], set[str], set[str]]:
    """Return the names bound to the importlib module, its loaders, and sys."""
    importlib_names: set[str] = set()
    loaders: set[str] = set()
    sys_names: set[str] = set()
    for node in nodes:
        if isinstance(node, ast.Import):
            for alias in node.names:
                top = alias.name.partition(".")[0]
                bound = alias.asname or top
                if alias.name == _IMPORTLIB or (
                    alias.asname is None and top == _IMPORTLIB
                ):
                    importlib_names.add(bound)
                elif alias.name == _IMPORTLIB + ".util" and alias.asname:
                    importlib_names.add(alias.asname)
                if alias.name == _SYS:
                    sys_names.add(alias.asname or _SYS)
        elif isinstance(node, ast.ImportFrom) and node.level == 0:
            for alias in node.names:
                if node.module == _IMPORTLIB and alias.name in _LOADER_FUNCTIONS:
                    loaders.add(alias.asname or alias.name)
                elif node.module == _IMPORTLIB and alias.name == "util":
                    importlib_names.add(alias.asname or alias.name)
    return importlib_names, loaders, sys_names


def _literal_load(call: ast.Call, line: int) -> DynamicLoad:
    """Judge a direct loader call: one positional literal absolute module and nothing else."""
    if call.keywords or len(call.args) != 1:
        return DynamicLoad(
            line, problem="a dynamic load takes exactly one literal module"
        )
    (argument,) = call.args
    if not isinstance(argument, ast.Constant) or type(argument.value) is not str:
        return DynamicLoad(line, problem="a dynamic load names a non-literal module")
    module = argument.value
    if module.startswith(".") or not module:
        return DynamicLoad(
            line, problem=f"a dynamic load names relative module {module!r}"
        )
    return DynamicLoad(line, module=module)


def _forbidden_import(node: ast.AST) -> str | None:
    modules: list[str] = []
    if isinstance(node, ast.Import):
        modules = [alias.name for alias in node.names]
    elif isinstance(node, ast.ImportFrom) and node.level == 0 and node.module:
        modules = [node.module, *(f"{node.module}.{a.name}" for a in node.names)]
        if node.module == _SYS and any(a.name == "modules" for a in node.names):
            return "sys.modules"
    for module in modules:
        for forbidden in _FORBIDDEN_MODULES:
            if module == forbidden or module.startswith(forbidden + "."):
                return module
    return None


_Names = tuple[set[str], set[str], set[str]]


def _is_direct_load(func: ast.expr, names: _Names) -> bool:
    importlib_names, loaders, _sys_names = names
    if isinstance(func, ast.Name):
        return func.id in loaders or func.id == "__import__"
    return (
        isinstance(func, ast.Attribute)
        and func.attr == "import_module"
        and isinstance(func.value, ast.Name)
        and func.value.id in importlib_names
    )


@dataclass(frozen=True)
class _Kinds:
    """A module's nodes of the kinds dynamic loading can hide in."""

    imports: tuple[ast.Import | ast.ImportFrom, ...]
    calls: tuple[ast.Call, ...]
    attributes: tuple[ast.Attribute, ...]
    names: tuple[ast.Name, ...]
    strings: tuple[ast.Constant, ...]


def _kinds(nodes: Sequence[ast.AST]) -> _Kinds:
    imports: list[ast.Import | ast.ImportFrom] = []
    calls: list[ast.Call] = []
    attributes: list[ast.Attribute] = []
    names: list[ast.Name] = []
    strings: list[ast.Constant] = []
    for node in nodes:
        kind = type(node)
        if kind is ast.Name:
            names.append(cast(ast.Name, node))
        elif kind is ast.Attribute:
            attributes.append(cast(ast.Attribute, node))
        elif kind is ast.Call:
            calls.append(cast(ast.Call, node))
        elif kind is ast.Constant and type(cast(ast.Constant, node).value) is str:
            strings.append(cast(ast.Constant, node))
        elif kind is ast.Import or kind is ast.ImportFrom:
            imports.append(cast(ast.Import | ast.ImportFrom, node))
    return _Kinds(
        tuple(imports), tuple(calls), tuple(attributes), tuple(names), tuple(strings)
    )


def _import_problems(kinds: _Kinds) -> list[DynamicLoad]:
    found = []
    for node in kinds.imports:
        forbidden = _forbidden_import(node)
        if forbidden is not None:
            found.append(DynamicLoad(node.lineno, problem=f"imports {forbidden!r}"))
        if isinstance(node, ast.ImportFrom) and node.level == 0:
            for alias in node.names:
                if alias.name in _FORBIDDEN_ATTRIBUTES - _LOADER_FUNCTIONS:
                    found.append(
                        DynamicLoad(node.lineno, problem=f"imports {alias.name!r}")
                    )
    return found


def _call_and_attribute_problems(
    kinds: _Kinds, names: _Names, permitted: set[int]
) -> tuple[list[DynamicLoad], set[int]]:
    """Judge direct loader calls and attribute uses; return the allowed module bases."""
    importlib_names, _loaders, sys_names = names
    modules = importlib_names | sys_names
    found: list[DynamicLoad] = []
    bases: set[int] = set()
    for call in kinds.calls:
        if _is_direct_load(call.func, names):
            found.append(_literal_load(call, call.lineno))
            permitted.add(id(call.func))
            if isinstance(call.func, ast.Attribute):
                bases.add(id(call.func.value))
    for node in kinds.attributes:
        value = node.value
        if type(value) is ast.Name and value.id in modules:
            bases.add(id(value))
            if value.id in sys_names and node.attr == "modules":
                found.append(DynamicLoad(node.lineno, problem="references sys.modules"))
            elif node.attr.startswith("__") and id(node) not in permitted:
                # `sys.__dict__` and `sys.__getattribute__` reach every member.
                found.append(
                    DynamicLoad(
                        node.lineno,
                        problem=f"references module attribute {value.id}.{node.attr}",
                    )
                )
                continue
        if node.attr in _FORBIDDEN_ATTRIBUTES and id(node) not in permitted:
            found.append(
                DynamicLoad(node.lineno, problem=f"references attribute {node.attr!r}")
            )
    return found, bases


def _name_and_string_problems(
    kinds: _Kinds, names: _Names, permitted: set[int], bases: set[int]
) -> list[DynamicLoad]:
    importlib_names, loaders, sys_names = names
    modules = importlib_names | sys_names
    found = []
    for node in kinds.names:
        if id(node) in permitted:
            continue
        if node.id in loaders or node.id in _FORBIDDEN_NAMES:
            found.append(DynamicLoad(node.lineno, problem=f"references {node.id!r}"))
        elif (
            node.id in modules
            and id(node) not in bases
            and isinstance(node.ctx, ast.Load)
        ):
            found.append(
                DynamicLoad(node.lineno, problem=f"uses module {node.id!r} as a value")
            )
    for node in kinds.strings:
        if node.value in _FORBIDDEN_STRINGS:
            found.append(
                DynamicLoad(node.lineno, problem=f"names {node.value!r} in a string")
            )
    return found


@functools.lru_cache(maxsize=1024)
def dynamic_loads(name: str, source: ModuleSource) -> tuple[DynamicLoad, ...]:
    """Return every dynamic-loading construct in a module, in line order.

    A direct call of ``importlib.import_module``, of a name ``from importlib
    import import_module`` binds, or of ``__import__`` with one literal
    absolute module is a permitted load of that module. Every other use of
    a loader, of a name bound to ``importlib``, ``importlib.util``, or
    ``sys`` other than an attribute access the rules allow, of ``exec``,
    ``eval``, ``compile``, the import machinery, a forbidden module, or a
    string naming any of them is a problem. Raises
    CheckerSourceUnavailable when the source does not parse.
    """
    nodes = _nodes(name, source)
    kinds = _kinds(nodes)
    names = _bindings(kinds.imports)
    permitted: set[int] = set()
    found = _import_problems(kinds)
    calls, bases = _call_and_attribute_problems(kinds, names, permitted)
    found += calls
    found += _name_and_string_problems(kinds, names, permitted, bases)
    return tuple(sorted(found, key=lambda d: (d.line, d.module or "", d.problem or "")))


class _Closure:
    """The walk behind `source_closure`: classify, follow, and digest imports."""

    def __init__(self, policy: SourcePolicy, reader: SourceReader) -> None:
        self._policy = policy
        self._reader = reader
        self._seen: set[str] = set()
        self._pending: list[str] = []
        self.digests: dict[str, str] = {}

    def kind(self, module: str) -> str:
        follow = self._policy.follow
        if _within(module, follow):
            return _FOLLOW
        if _is_stdlib(module) or _within(module, self._policy.external):
            return _ENVIRONMENT
        if any(_within(package, (module,)) for package in follow):
            return _PARENT
        return _UNDECLARED

    def enqueue(self, module: str) -> None:
        """Follow a module and every package Python runs to import it."""
        for name in (*_ancestors(module), module):
            if name in self._seen:
                continue
            self._seen.add(name)
            if self.kind(name) == _FOLLOW:
                self._pending.append(name)
            elif not _codeless(self._reader(name)):
                raise CheckerSourceUnavailable(
                    f"package {name!r} encloses the followed packages and has code"
                )

    def run(self) -> None:
        while self._pending:
            name = self._pending.pop()
            source = self._reader(name)
            if _codeless(source):
                continue
            assert source is not None
            self.digests[name] = _sha256(semantic_text(name, source))
            for statement in import_statements(name, source):
                self._statement(name, source, statement)
            if name != _CHECKER_MODULE:
                for load in dynamic_loads(name, source):
                    self._dynamic(name, load)

    def _fail(self, name: str, statement: ImportStatement, why: str) -> NoReturn:
        raise CheckerSourceUnavailable(f"module {name!r} line {statement.line}: {why}")

    def _dynamic(self, name: str, load: DynamicLoad) -> None:
        """Fail closed on a dynamic load unless it names a declared module literally."""
        if load.problem is not None:
            raise CheckerSourceUnavailable(
                f"module {name!r} line {load.line}: dynamic loading: {load.problem}"
            )
        if load.module not in self._policy.dynamic:
            raise CheckerSourceUnavailable(
                f"module {name!r} line {load.line}: dynamically loads"
                f" {load.module!r}, which TargetSources.dynamic does not declare"
            )

    def _statement(
        self, name: str, source: ModuleSource, statement: ImportStatement
    ) -> None:
        module = statement.module
        if module is None:
            self._fail(name, statement, "relative import climbs above the top level")
        kind = self.kind(module)
        if kind == _ENVIRONMENT:
            return
        if kind == _UNDECLARED:
            self._fail(
                name,
                statement,
                f"imports {module!r}, which is neither followed, the standard "
                "library, nor declared external",
            )
        if kind == _PARENT and (not statement.names or statement.star):
            self._fail(
                name,
                statement,
                f"imports the enclosing package {module!r}, which can reach "
                "unfollowed code; import the module itself",
            )
        if kind == _FOLLOW:
            self.enqueue(module)
            imported = self._reader(module)
            if statement.star and (imported is None or imported.is_package):
                self._fail(name, statement, f"star-imports package {module!r}")
        for member in statement.names:
            self._member(name, statement, module, kind, f"{module}.{member}")

    def _member(
        self, name: str, statement: ImportStatement, module: str, kind: str, child: str
    ) -> None:
        """Follow a name a ``from`` statement binds when it is a module."""
        child_kind = self.kind(child)
        if child_kind == _ENVIRONMENT:
            return
        exists = self._reader(child) is not None
        if child_kind == _FOLLOW:
            if exists:
                self.enqueue(child)
            elif kind == _PARENT:
                self._fail(name, statement, f"imports {child!r}, which has no source")
            return
        if kind == _PARENT or exists:
            self._fail(
                name,
                statement,
                f"imports {child!r} from {module!r}: not followed, the standard "
                "library, nor declared external",
            )


def source_closure(
    roots: Iterable[str],
    *,
    policy: SourcePolicy = CHECKER_POLICY,
    reader: SourceReader = read_module_source,
) -> Mapping[str, str]:
    """Return the digest of every root module and every module its imports reach.

    Every root must lie in `policy.follow` and name a module with readable
    source. Each import statement of a followed module must resolve to a
    followed module, the standard library, or a declared external package;
    a name a ``from`` statement binds is followed when it is a module.
    Enclosing packages of the followed ones (``eden``, ``eden.integration``)
    must have no code, may not be imported themselves, and may only yield
    followed or external submodules. Relative imports above the top level,
    star imports of packages, undeclared imports, and dynamic loading other
    than a literal load of a `policy.dynamic` module (each also a root)
    fail. A module with
    no code (a generated, empty ``__init__.py``) is not digested, so a
    packaged binary and a source checkout agree. No external package may
    overlap a followed one. Raises CheckerSourceUnavailable otherwise.
    """
    for external in policy.external:
        for package in policy.follow:
            if _within(external, (package,)) or _within(package, (external,)):
                raise CheckerSourceUnavailable(
                    f"external package {external!r} overlaps followed {package!r}"
                )
    for module in policy.dynamic:
        if not _within(module, policy.follow):
            raise CheckerSourceUnavailable(
                f"dynamic module {module!r} is outside the followed packages"
            )
    closure = _Closure(policy, reader)
    for root in sorted({*roots, *policy.dynamic}):
        if not _within(root, policy.follow):
            raise CheckerSourceUnavailable(
                f"module {root!r} is outside the followed packages {policy.follow}"
            )
        if reader(root) is None:
            raise CheckerSourceUnavailable(f"root module {root!r} has no source")
        closure.enqueue(root)
    closure.run()
    return dict(sorted(closure.digests.items()))
