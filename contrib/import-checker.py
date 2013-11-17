import ast
import os
import sys

def dotted_name_of_path(path):
    """Given a relative path to a source file, return its dotted module name.


    >>> dotted_name_of_path('mercurial/error.py')
    'mercurial.error'
    """
    parts = path.split('/')
    parts[-1] = parts[-1][:-3] # remove .py
    return '.'.join(parts)


def list_stdlib_modules():
    """List the modules present in the stdlib.

    >>> mods = set(list_stdlib_modules())
    >>> 'BaseHTTPServer' in mods
    True

    os.path isn't really a module, so it's missing:

    >>> 'os.path' in mods
    False

    sys requires special treatment, because it's baked into the
    interpreter, but it should still appear:

    >>> 'sys' in mods
    True

    >>> 'collections' in mods
    True

    >>> 'cStringIO' in mods
    True
    """
    for m in sys.builtin_module_names:
        yield m
    # These modules only exist on windows, but we should always
    # consider them stdlib.
    for m in ['msvcrt', '_winreg']:
        yield m
    # These get missed too
    for m in 'ctypes', 'email':
        yield m
    yield 'builtins' # python3 only
    for libpath in sys.path:
        # We want to walk everything in sys.path that starts with
        # either sys.prefix or sys.exec_prefix.
        if not (libpath.startswith(sys.prefix)
                or libpath.startswith(sys.exec_prefix)):
            continue
        if 'site-packages' in libpath:
            continue
        for top, dirs, files in os.walk(libpath):
            for name in files:
                if name == '__init__.py':
                    continue
                if not (name.endswith('.py') or name.endswith('.so')):
                    continue
                full_path = os.path.join(top, name)
                if 'site-packages' in full_path:
                    continue
                rel_path = full_path[len(libpath) + 1:]
                mod = dotted_name_of_path(rel_path)
                yield mod

stdlib_modules = set(list_stdlib_modules())

def imported_modules(source, ignore_nested=False):
    """Given the source of a file as a string, yield the names
    imported by that file.

    Args:
      source: The python source to examine as a string.
      ignore_nested: If true, import statements that do not start in
                     column zero will be ignored.

    Returns:
      A list of module names imported by the given source.

    >>> sorted(imported_modules(
    ...         'import foo ; from baz import bar; import foo.qux'))
    ['baz.bar', 'foo', 'foo.qux']
    >>> sorted(imported_modules(
    ... '''import foo
    ... def wat():
    ...     import bar
    ... ''', ignore_nested=True))
    ['foo']
    """
    for node in ast.walk(ast.parse(source)):
        if ignore_nested and getattr(node, 'col_offset', 0) > 0:
            continue
        if isinstance(node, ast.Import):
            for n in node.names:
                yield n.name
        elif isinstance(node, ast.ImportFrom):
            prefix = node.module + '.'
            for n in node.names:
                yield prefix + n.name

def verify_stdlib_on_own_line(source):
    """Given some python source, verify that stdlib imports are done
    in separate statements from relative local module imports.

    Observing this limitation is important as it works around an
    annoying lib2to3 bug in relative import rewrites:
    http://bugs.python.org/issue19510.

    >>> list(verify_stdlib_on_own_line('import sys, foo'))
    ['mixed stdlib and relative imports:\\n   foo, sys']
    >>> list(verify_stdlib_on_own_line('import sys, os'))
    []
    >>> list(verify_stdlib_on_own_line('import foo, bar'))
    []
    """
    for node in ast.walk(ast.parse(source)):
        if isinstance(node, ast.Import):
            from_stdlib = {}
            for n in node.names:
                from_stdlib[n.name] = n.name in stdlib_modules
            num_std = len([x for x in from_stdlib.values() if x])
            if num_std not in (len(from_stdlib.values()), 0):
                yield ('mixed stdlib and relative imports:\n   %s' %
                       ', '.join(sorted(from_stdlib.iterkeys())))

class CircularImport(Exception):
    pass


def cyclekey(names):
    return tuple(sorted(set(names)))

def check_one_mod(mod, imports, path=None, ignore=None):
    if path is None:
        path = []
    if ignore is None:
        ignore = []
    path = path + [mod]
    for i in sorted(imports.get(mod, [])):
        if i not in stdlib_modules:
            i = mod.rsplit('.', 1)[0] + '.' + i
        if i in path:
            firstspot = path.index(i)
            cycle = path[firstspot:] + [i]
            if cyclekey(cycle) not in ignore:
                raise CircularImport(cycle)
            continue
        check_one_mod(i, imports, path=path, ignore=ignore)

def rotatecycle(cycle):
    """arrange a cycle so that the lexicographically first module listed first

    >>> rotatecycle(['foo', 'bar', 'foo'])
    ['bar', 'foo', 'bar']
    """
    lowest = min(cycle)
    idx = cycle.index(lowest)
    return cycle[idx:] + cycle[1:idx] + [lowest]

def find_cycles(imports):
    """Find cycles in an already-loaded import graph.

    >>> imports = {'top.foo': ['bar', 'os.path', 'qux'],
    ...            'top.bar': ['baz', 'sys'],
    ...            'top.baz': ['foo'],
    ...            'top.qux': ['foo']}
    >>> print '\\n'.join(sorted(find_cycles(imports)))
    top.bar -> top.baz -> top.foo -> top.bar -> top.bar
    top.foo -> top.qux -> top.foo -> top.foo
    """
    cycles = {}
    for mod in sorted(imports.iterkeys()):
        try:
            check_one_mod(mod, imports, ignore=cycles)
        except CircularImport, e:
            cycle = e.args[0]
            cycles[cyclekey(cycle)] = ' -> '.join(rotatecycle(cycle))
    return cycles.values()

def _cycle_sortkey(c):
    return len(c), c

def main(argv):
    if len(argv) < 2:
        print 'Usage: %s file [file] [file] ...'
        return 1
    used_imports = {}
    any_errors = False
    for source_path in argv[1:]:
        f = open(source_path)
        modname = dotted_name_of_path(source_path)
        src = f.read()
        used_imports[modname] = sorted(
            imported_modules(src, ignore_nested=True))
        for error in verify_stdlib_on_own_line(src):
            any_errors = True
            print source_path, error
        f.close()
    cycles = find_cycles(used_imports)
    if cycles:
        firstmods = set()
        for c in sorted(cycles, key=_cycle_sortkey):
            first = c.split()[0]
            # As a rough cut, ignore any cycle that starts with the
            # same module as some other cycle. Otherwise we see lots
            # of cycles that are effectively duplicates.
            if first in firstmods:
                continue
            print 'Import cycle:', c
            firstmods.add(first)
        any_errors = True
    return not any_errors

if __name__ == '__main__':
    sys.exit(int(main(sys.argv)))
