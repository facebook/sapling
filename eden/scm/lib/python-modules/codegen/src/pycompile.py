# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""
Compile pure Python modules recursively.

Input (env):
- ROOT_MODULES: Space-separated root module names.
- SYS_PATH0: Path to be inserted to sys.path[0].
  Usually it's the directory that contains the modules to compile.

Output (to stdout):
The first two line is the Python version major (1st line), minor (2nd line).
Then, for each module, print 5 lines:
- Module name (foo.bar).
- File path in HEX, (/path/to/foo/bar.py).
- Source code in HEX, one line.
- Compiled bytecode in HEX, one line.
- Empty line.

Using HEX because:
- Python's binary data serialization from stdlib (marsh, pickle) are tricky to
  be consumed by Rust.
- HEX is simple for this use-case.

Sending back to Rust, instead of generating the `.rs` file directly because
Python source code is a lot of data (20MB), and the "desired" compression
algorithm zstd is not yet in Python stdlib.
"""

import binascii
import glob
import importlib.util
import marshal
import os
import sys

dirname = os.path.dirname


def module_name_from_rel_path(path):
    """'foo/bar.py' -> 'foo.bar'; 'foo/__init__.py' -> 'foo'"""
    if os.path.basename(path) == "__init__.py":
        path = dirname(path)
    else:
        # strip ".py"
        path = path[:-3]
    return path.replace(os.path.sep, "/").replace("/", ".")


def find_modules(modules):
    """Find modules recursively. Return [(module_name, path)]."""
    result = []
    for root_module_name in modules:
        locations = []
        if root_module_name == "std":
            root_module_name = ""
            if os.__file__ and os.path.exists(os.__file__):
                locations.append(dirname(os.__file__))
        else:
            # Find the module without importing it.
            spec = importlib.util.find_spec(root_module_name)
            # Examples:
            #   >>> importlib.util.find_spec('sapling')
            #   ModuleSpec(name='sapling', origin='.../sapling/__init__.py',
            #              submodule_search_locations=['.../sapling'])
            #   >>> importlib.util.find_spec('sapling.mercurial')
            #   ModuleSpec(name='sapling.ext', origin='.../sapling/ext/__init__.py',
            #              submodule_search_locations=['.../sapling/ext'])
            #   >>> importlib.util.find_spec('saplingercurial.revset')
            #   ModuleSpec(name='sapling.revset', origin='.../sapling/revset.py')
            if spec is None:
                raise RuntimeError(
                    f"cannot locate Python module {root_module_name} in {sys.path}"
                )
            locations += spec.submodule_search_locations or []
        if locations:
            # a directory - scan it recursively
            for location in locations:
                for path in glob.glob(
                    os.path.join(location, "**", "*.py"), recursive=True
                ):
                    rel_path = os.path.relpath(path, location)
                    rel_module_name = module_name_from_rel_path(rel_path)
                    module_name = ".".join(
                        filter(None, [root_module_name, rel_module_name])
                    )
                    result.append((module_name, path))
        elif spec.origin and os.path.exists(spec.origin):
            # a single file
            result.append((root_module_name, spec.origin))
    return result


def hex(s: bytes) -> str:
    return binascii.hexlify(s).decode()


def main():
    root_modules = (os.getenv("ROOT_MODULES") or "").split()
    sys_path0 = os.getenv("SYS_PATH0")
    if sys_path0:
        sys.path[0:0] = [sys_path0]

    print(sys.version_info.major)
    print(sys.version_info.minor)

    for module_name, path in find_modules(root_modules):
        with open(path, "rb") as f:
            source = f.read()
        if module_name == "linecache":
            # patch linecache so it can read static modules from bindings.
            # stdlib doctest does something similar.
            source += rb"""
_orig_updatecache = updatecache

def updatecache(filename, module_globals=None):
    prefix = "<static:"
    if filename.startswith(prefix) and filename.endswith(">"):
        name = filename[len(prefix):-1]
        try:
            import bindings
        except ImportError:
            pass
        else:
            source = bindings.modules.get_source(name)
            if source is not None:
                buf = source.asref().tobytes()
                lines = buf.decode().splitlines(True)
                # size, mtime, lines, fullname
                cache[filename] = (len(buf), None, lines, filename)
                return lines
    return _orig_updatecache(filename, module_globals)
"""

        try:
            code = compile(source, f"<static:{module_name}>", "exec")
        except SyntaxError:
            # Some ".py" files in stdlib won't compile:
            # - lib2to3.tests contains tests in Python 2 syntax.
            # - test.bad_coding is not utf-8.
            if any(module_name.startswith(s) for s in ("test.", "lib2to3.tests.")):
                continue
            raise
        print(module_name)
        print(hex(path.encode()))
        print(hex(source))
        bytecode = marshal.dumps(code)
        print(hex(bytecode))
        print()


main()
