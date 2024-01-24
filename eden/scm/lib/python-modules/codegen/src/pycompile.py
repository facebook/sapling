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
- True if it's stdlib.
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
    name = os.path.basename(path)
    if name in ("__init__.py", "__init__.pyc"):
        path = dirname(path)
    else:
        # strip ".py" or ".pyc"
        path = path.rsplit(".", 1)[0]
    return path.replace(os.path.sep, "/").replace("/", ".")


def find_modules(modules):
    """Find modules recursively. Return [(module_name, path, source|None, bytecode|None)]."""
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
            #   >>> importlib.util.find_spec('saplingercurial.revset')
            #   ModuleSpec(name='sapling.revset', origin='.../sapling/revset.py')
            #
            # Zip examples:
            #   >>> spec = importlib.util.find_spec('xml')
            #   ModuleSpec(name='xml', origin='.../python311.zip/xml/__init__.pyc',
            #              submodule_search_locations=['.../python311.zip/xml'])
            #   >>> importlib.util.find_spec('xml.dom.minidom')
            #   ModuleSpec(name='xml.dom.minidom', origin='.../python311.zip/xml/dom/minidom.pyc')

            if spec is None:
                raise RuntimeError(
                    f"cannot locate Python module {root_module_name} in {sys.path}"
                )
            locations += spec.submodule_search_locations or []
        is_zip = type(spec.loader).__name__ == "zipimporter"
        if locations:
            # a directory - scan it recursively
            if not is_zip:
                for location in locations:
                    for path in glob.glob(
                        os.path.join(location, "**", "*.py"), recursive=True
                    ):
                        rel_path = os.path.relpath(path, location)
                        rel_module_name = module_name_from_rel_path(rel_path)
                        module_name = ".".join(
                            filter(None, [root_module_name, rel_module_name])
                        )
                        with open(path, "rb") as f:
                            source = f.read()
                        result.append((module_name, path, source, None))
            else:
                # ex. '/usr/lib64/python311.zip'
                zip_path = spec.loader.archive
                for location in locations:
                    prefix = location[len(zip_path) + 1 :] + os.path.sep
                    for zip_file_path, info in spec.loader._files.items():
                        if not zip_file_path.startswith(prefix):
                            continue
                        if not any(zip_file_path.endswith(p) for p in (".py", ".pyc")):
                            continue
                        # do not use importlib.util.find_spec - it imports the
                        # parent module, which might fail on Windows when
                        # importing 'curses.ascii'.
                        zip_module_name = module_name_from_rel_path(zip_file_path)
                        data = spec.loader.get_data(zip_file_path)
                        if zip_file_path.endswith(".pyc"):
                            source = b""
                            # 16: magic (4B) + mtime (4B) + size (4B, >=3.3) + hash (4B, >=3.7)
                            # See: zipimport._unmarshal_code
                            # https://github.com/python/cpython/blob/3.10/Lib/zipimport.py#L677
                            code = data[16:]
                        else:
                            source = data
                            code = None
                        result.append(
                            (
                                zip_module_name,
                                os.path.sep.join((zip_path, zip_file_path)),
                                source,
                                code,
                            )
                        )
        elif spec.origin is not None:
            # not a package - a file on filesystem or inside a zip
            source = (spec.loader.get_source(spec.name) or "").encode()
            code_obj = spec.loader.get_code(spec.name)
            if code_obj:
                code = marshal.dumps(code_obj)
                result.append((root_module_name, spec.origin, source, code))

    return result


def hex(s: bytes) -> str:
    return binascii.hexlify(s).decode()


def default_root_modules() -> "list[str]":
    modules = ["sapling", "ghstack"]
    modules += STDLIB_MODULE_NAMES
    return modules


# use `debuglistpythonstd` on Windows and Linux to get the list of modules
STDLIB_MODULE_NAMES = [
    "__future__",
    "_collections_abc",
    "_compat_pickle",
    "_compression",
    "_sitebuiltins",
    "_strptime",
    "_weakrefset",
    "abc",
    "argparse",
    "ast",
    "asyncio",
    "base64",
    "bdb",
    "bisect",
    "bz2",
    "calendar",
    "cmd",
    "code",
    "codecs",
    "collections",
    "concurrent",
    "configparser",
    "contextlib",
    "contextvars",
    "copy",
    "copyreg",
    "ctypes",
    "curses",
    "dataclasses",
    "datetime",
    "dbm",
    "difflib",
    "dis",
    "doctest",
    "email",
    "encodings",
    "enum",
    "filecmp",
    "fnmatch",
    "ftplib",
    "functools",
    "genericpath",
    "getopt",
    "getpass",
    "gettext",
    "glob",
    "gzip",
    "hashlib",
    "heapq",
    "hmac",
    "http",
    "importlib",
    "inspect",
    "io",
    "json",
    "keyword",
    "linecache",
    "locale",
    "logging",
    "lzma",
    "mimetypes",
    "multiprocessing",
    "ntpath",
    "nturl2path",
    "opcode",
    "operator",
    "os",
    "pathlib",
    "pdb",
    "pickle",
    "pipes",
    "platform",
    "posixpath",
    "pprint",
    "queue",
    "quopri",
    "random",
    "re",
    "reprlib",
    "selectors",
    "shlex",
    "shutil",
    "signal",
    "site",
    "smtplib",
    "socket",
    "socketserver",
    "sqlite3",
    "sre_compile",
    "sre_constants",
    "sre_parse",
    "ssl",
    "stat",
    "string",
    "struct",
    "subprocess",
    "tarfile",
    "tempfile",
    "textwrap",
    "threading",
    "token",
    "tokenize",
    "traceback",
    "tty",
    "types",
    "typing",
    "unittest",
    "urllib",
    "uu",
    "uuid",
    "warnings",
    "weakref",
    "zipfile",
]


def main():
    root_modules = (os.getenv("ROOT_MODULES") or "").split() or default_root_modules()
    sys_path0 = os.getenv("SYS_PATH0")
    if sys_path0:
        sys.path[0:0] = [sys_path0]

    print(sys.version_info.major)
    print(sys.version_info.minor)
    stdlib_names = set(STDLIB_MODULE_NAMES)

    for module_name, path, source, code in find_modules(root_modules):
        if code is None:
            assert source is not None
            try:
                code_obj = compile(source, f"static:{module_name}", "exec")
            except SyntaxError:
                # Some ".py" files in stdlib won't compile:
                # - lib2to3.tests contains tests in Python 2 syntax.
                # - test.bad_coding is not utf-8.
                if any(module_name.startswith(s) for s in ("test.", "lib2to3.tests.")):
                    continue
                raise
            code = marshal.dumps(code_obj)
        print(module_name)
        print(hex(path.encode()))
        print(hex(source.decode(errors="replace").encode()))
        print(hex(code))
        is_stdlib = module_name.split(".", 1)[0] in stdlib_names
        print(is_stdlib)
        print()


main()
