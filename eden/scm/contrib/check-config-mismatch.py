#!sl dbsh
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import ast
import collections
import dataclasses
import glob
import json
import subprocess

from sapling import util

_unset = object()
dynamicdefault = None

CFG_ALLOW_ANY_SOURCE_COMMIT = "allow-any-source-commit"
pushrebasemarker = "__pushrebase_processed__"
rebasemsg = "x"
pathformat = "x"
mempathformat = "x"
_DEFAULT_CACHE_SIZE = 10000
TREE_DEPTH_MAX = 2**16


def configwith(convert, section, name, default=_unset, desc=None):
    return (f"{section}.{name}", desc or convert.__name__, default)


def config(section, name, default=_unset):
    return configwith("", section, name, default, desc="str")


def configint(section, name, default=_unset):
    return configwith("", section, name, default, desc="int")


def configbool(section, name, default=_unset):
    return configwith("", section, name, default, desc="bool")


def configbytes(section, name, default=_unset):
    return configwith("", section, name, default, desc="bytes")


def configpath(section, name, default=_unset):
    return configwith("", section, name, default, desc="path")


def coreconfigitem(section, name, default=_unset, alias=None, generic=None, priority=0):
    if callable(default):
        default = default()
    kind = _kind_from_value(default)
    return configwith("", section, name, default, desc=kind)


def configitem(section, name, default=_unset, alias=None, generic=None):
    return coreconfigitem(section, name, default)


def _kind_from_value(value):
    if value is _unset:
        return "unset"
    kind = "unknown"
    for t in [float, int, str, list, bool, None.__class__]:
        if isinstance(value, t):
            kind = t.__name__
    return kind


configitem.dynamicdefault = _unset


@dataclasses.dataclass
class ConfigItem:
    name: str
    kind: str
    default: object
    loc: str

    def can_be_bytes(self):
        if isinstance(self.default, int):
            return True
        try:
            util.sizetoint(self.default)
            return True
        except Exception:
            return False

    def can_be_bool(self):
        if isinstance(self.default, bool):
            return True
        try:
            util.parsebool(self.default)
            return True
        except Exception:
            return False


def scan(method):
    method_name = method.__name__
    paths = glob.glob("**/*.py", recursive=True)
    prefix = "$UI."
    if method in {configitem, coreconfigitem}:
        prefix = ""
    out = subprocess.check_output(
        ["ast-grep", "run", "-p", f"{prefix}{method_name}($$$ARGS)", "--json", *paths]
    )
    # Example:
    #
    #   [{"text": "ui.configwith(float, \"progress\", \"estimateinterval\")",
    #     "range": { "byteOffset": { "start": 11581, "end": 11633 },
    #       "start": { "line": 389, "column": 33 },
    #       "end": { "line": 389, "column": 85 } },
    #     "file": "./sapling/progress.py",
    #     "lines": "        self._estimateinterval = ui.configwith(float, \"progress\", \"estimateinterval\")",
    #     "language": "Python",
    #     "metaVariables": {
    #       "single": {
    #         "UI": { "text": "ui",
    #           "range": { "byteOffset": { "start": 11581, "end": 11583 },
    #             "start": { "line": 389, "column": 33 }, "end": { "line": 389, "column": 35 } } } },
    #       "multi": {
    #         "ARGS": [
    #           { "text": "float",
    #             "range": { "byteOffset": { "start": 11595, "end": 11600 }, "start": { "line": 389, "column": 47 }, "end": { "line": 389, "column": 52 } } },
    #           { "text": ",",
    #             "range": { "byteOffset": { "start": 11600, "end": 11601 }, "start": { "line": 389, "column": 52 }, "end": { "line": 389, "column": 53 } } },
    #           { "text": "\"progress\"",
    #             "range": { "byteOffset": { "start": 11602, "end": 11612 }, "start": { "line": 389, "column": 54 }, "end": { "line": 389, "column": 64 } } },
    #           { "text": ",",
    #             "range": { "byteOffset": { "start": 11612, "end": 11613 }, "start": { "line": 389, "column": 64 }, "end": { "line": 389, "column": 65 } } },
    #           { "text": "\"estimateinterval\"",
    #             "range": { "byteOffset": { "start": 11614, "end": 11632 }, "start": { "line": 389, "column": 66 }, "end": { "line": 389, "column": 84 } } } ] },
    #       "transformed": {} } },
    #       ... ]
    decoded = json.loads(out.decode())
    section_arg_index = 0
    if method == "configwith":
        section_arg_index = 1
    for obj in decoded:
        args = "".join(
            arg["text"]
            for arg in obj["metaVariables"]["multi"]["ARGS"]
            if not arg["text"].startswith("#")
        )
        loc = f"{obj['file']}:{obj['range']['start']['line']}"
        code = f"{method_name}({args})"
        try:
            name, kind, default = eval(code)
        except (NameError, TypeError):
            print(f"  ignored({method_name}): {code} at {loc}")
            continue
        yield ConfigItem(name, kind, default, loc)


print("Scanning registered configs")
registered = {}

for cfg in scan(coreconfigitem):
    assert cfg.name not in registered
    if cfg.default is not _unset:
        registered[cfg.name] = cfg

for cfg in scan(configitem):
    assert cfg.name not in registered
    if cfg.default is not _unset:
        registered[cfg.name] = cfg


print("Scanning referred configs")
referred = collections.defaultdict(list)

for method in [config, configbool, configint, configpath, configbytes, configwith]:
    for cfg in scan(method):
        referred[cfg.name].append(cfg)

print("Checking types")
for key, registered_cfg in registered.items():
    registered_kind = registered_cfg.kind
    problems = []
    for cfg in referred.get(key) or []:
        method_kind = cfg.kind
        if cfg.default is not _unset:
            default_kind = _kind_from_value(cfg.default)
            if default_kind != registered_kind:
                problems.append(
                    f"  mismatched default type: {default_kind} (default={cfg.default}) at {cfg.loc}"
                )
            elif cfg.default != registered_cfg.default:
                problems.append(
                    f"  mismatched default value: {cfg.default} at {cfg.loc}"
                )
        if method_kind != registered_kind:
            is_bad = True
            if method_kind == "bytes" and registered_cfg.can_be_bytes():
                is_bad = False
            elif method_kind == "float" and registered_kind == "int":
                is_bad = False
            elif method_kind == "bool" and registered_cfg.can_be_bool():
                is_bad = False
            if registered_kind == "NoneType":
                if method_kind == "bool":
                    if cfg.default is None:
                        is_bad = False
                else:
                    is_bad = False
            if is_bad:
                problems.append(
                    f"  mismatched config method: {method_kind} at {cfg.loc}"
                )
    if problems:
        print(registered_cfg.name)
        print(
            f" registered as {registered_kind} (default={registered_cfg.default}) at {registered_cfg.loc}"
        )
        print("\n".join(problems))
