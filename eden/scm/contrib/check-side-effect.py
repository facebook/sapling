#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import ast
import sys


class SideEffectVisitor(ast.NodeVisitor):
    def __init__(self, code, filename):
        self._lines = code.splitlines()
        self._filename = filename
        self._filename_printed = False

    def _maybe_print_filename(self):
        if not self._filename_printed:
            self._filename_printed = True
            print(f"{self._filename}")

    def _print_line(self, lineno, reason):
        self._maybe_print_filename()
        print(f"{str(lineno).rjust(5)}:{reason.rjust(7)}: {self._lines[lineno - 1]}")

    def visit_FunctionDef(self, node):
        # Don't traverse into the body of functions
        pass

    def visit_AsyncFunctionDef(self, node):
        # Don't traverse into the body of async functions
        pass

    def visit_ClassDef(self, node):
        # Don't traverse into the body of classes
        pass

    def visit_Assign(self, node):
        # Check for top-level assignments which might change global state
        # We only care about "Attribute" assignments like `sys.foo = 1`,
        # not local "global" variables like `CACHE_SIZE = 2`.
        for target in node.targets:
            if isinstance(target, (ast.Attribute,)) and isinstance(
                target.ctx, ast.Store
            ):
                self._print_line(node.lineno, "assign")

    def visit_Call(self, node):
        if not is_function_ok(node.func):
            self._print_line(node.lineno, "  call")


LOOKING_SAFE_FUNCTIONS = {"_", "re.compile", "coreconfigitem", "configitem"}


def is_function_ok(func):
    func_name = to_str(func)
    return func_name in LOOKING_SAFE_FUNCTIONS


def to_str(node):
    # convert ast 'foo.bar.baz' to string 'foo.bar.baz'
    if isinstance(node, ast.Attribute):
        return ".".join((to_str(node.value), node.attr))
    elif isinstance(node, ast.Name):
        return node.id
    else:
        return "?"


def check_for_side_effects(filename):
    with open(filename, "r") as f:
        code = f.read()

    tree = ast.parse(code)
    visitor = SideEffectVisitor(code, filename)
    visitor.visit(tree)


def main():
    if len(sys.argv) < 2:
        print(f"Usage: {sys.argv[0]} file1.py [file2.py ...]")
        sys.exit(1)

    for filepath in sys.argv[1:]:
        check_for_side_effects(filepath)


if __name__ == "__main__":
    main()
