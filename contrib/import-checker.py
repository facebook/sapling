import ast
import os
import sys

# Import a minimal set of stdlib modules needed for list_stdlib_modules()
# to work when run from a virtualenv.  The modules were chosen empirically
# so that the return value matches the return value without virtualenv.
import BaseHTTPServer
import zlib

def dotted_name_of_path(path, trimpure=False):
    """Given a relative path to a source file, return its dotted module name.

    >>> dotted_name_of_path('mercurial/error.py')
    'mercurial.error'
    >>> dotted_name_of_path('mercurial/pure/parsers.py', trimpure=True)
    'mercurial.parsers'
    >>> dotted_name_of_path('zlibmodule.so')
    'zlib'
    """
    parts = path.split('/')
    parts[-1] = parts[-1].split('.', 1)[0] # remove .py and .so and .ARCH.so
    if parts[-1].endswith('module'):
        parts[-1] = parts[-1][:-6]
    if trimpure:
        return '.'.join(p for p in parts if p != 'pure')
    return '.'.join(parts)

def fromlocalfunc(modulename, localmods):
    """Get a function to examine which locally defined module the
    target source imports via a specified name.

    `modulename` is an `dotted_name_of_path()`-ed source file path,
    which may have `.__init__` at the end of it, of the target source.

    `localmods` is a dict (or set), of which key is an absolute
    `dotted_name_of_path()`-ed source file path of locally defined (=
    Mercurial specific) modules.

    This function assumes that module names not existing in
    `localmods` are ones of Python standard libarary.

    This function returns the function, which takes `name` argument,
    and returns `(absname, dottedpath, hassubmod)` tuple if `name`
    matches against locally defined module. Otherwise, it returns
    False.

    It is assumed that `name` doesn't have `.__init__`.

    `absname` is an absolute module name of specified `name`
    (e.g. "hgext.convert"). This can be used to compose prefix for sub
    modules or so.

    `dottedpath` is a `dotted_name_of_path()`-ed source file path
    (e.g. "hgext.convert.__init__") of `name`. This is used to look
    module up in `localmods` again.

    `hassubmod` is whether it may have sub modules under it (for
    convenient, even though this is also equivalent to "absname !=
    dottednpath")

    >>> localmods = {'foo.__init__': True, 'foo.foo1': True,
    ...              'foo.bar.__init__': True, 'foo.bar.bar1': True,
    ...              'baz.__init__': True, 'baz.baz1': True }
    >>> fromlocal = fromlocalfunc('foo.xxx', localmods)
    >>> # relative
    >>> fromlocal('foo1')
    ('foo.foo1', 'foo.foo1', False)
    >>> fromlocal('bar')
    ('foo.bar', 'foo.bar.__init__', True)
    >>> fromlocal('bar.bar1')
    ('foo.bar.bar1', 'foo.bar.bar1', False)
    >>> # absolute
    >>> fromlocal('baz')
    ('baz', 'baz.__init__', True)
    >>> fromlocal('baz.baz1')
    ('baz.baz1', 'baz.baz1', False)
    >>> # unknown = maybe standard library
    >>> fromlocal('os')
    False
    >>> fromlocal(None, 1)
    ('foo', 'foo.__init__', True)
    >>> fromlocal2 = fromlocalfunc('foo.xxx.yyy', localmods)
    >>> fromlocal2(None, 2)
    ('foo', 'foo.__init__', True)
    """
    prefix = '.'.join(modulename.split('.')[:-1])
    if prefix:
        prefix += '.'
    def fromlocal(name, level=0):
        # name is None when relative imports are used.
        if name is None:
            # If relative imports are used, level must not be absolute.
            assert level > 0
            candidates = ['.'.join(modulename.split('.')[:-level])]
        else:
            # Check relative name first.
            candidates = [prefix + name, name]

        for n in candidates:
            if n in localmods:
                return (n, n, False)
            dottedpath = n + '.__init__'
            if dottedpath in localmods:
                return (n, dottedpath, True)
        return False
    return fromlocal

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
    for m in 'fcntl', 'grp', 'pwd', 'termios':  # Unix only
        yield m
    stdlib_prefixes = set([sys.prefix, sys.exec_prefix])
    # We need to supplement the list of prefixes for the search to work
    # when run from within a virtualenv.
    for mod in (BaseHTTPServer, zlib):
        try:
            # Not all module objects have a __file__ attribute.
            filename = mod.__file__
        except AttributeError:
            continue
        dirname = os.path.dirname(filename)
        for prefix in stdlib_prefixes:
            if dirname.startswith(prefix):
                # Then this directory is redundant.
                break
        else:
            stdlib_prefixes.add(dirname)
    for libpath in sys.path:
        # We want to walk everything in sys.path that starts with
        # something in stdlib_prefixes. check-code suppressed because
        # the ast module used by this script implies the availability
        # of any().
        if not any(libpath.startswith(p) for p in stdlib_prefixes): # no-py24
            continue
        if 'site-packages' in libpath:
            continue
        for top, dirs, files in os.walk(libpath):
            for name in files:
                if name == '__init__.py':
                    continue
                if not (name.endswith('.py') or name.endswith('.so')
                        or name.endswith('.pyd')):
                    continue
                full_path = os.path.join(top, name)
                if 'site-packages' in full_path:
                    continue
                rel_path = full_path[len(libpath) + 1:]
                mod = dotted_name_of_path(rel_path)
                yield mod

stdlib_modules = set(list_stdlib_modules())

def imported_modules(source, modulename, localmods, ignore_nested=False):
    """Given the source of a file as a string, yield the names
    imported by that file.

    Args:
      source: The python source to examine as a string.
      modulename: of specified python source (may have `__init__`)
      localmods: dict of locally defined module names (may have `__init__`)
      ignore_nested: If true, import statements that do not start in
                     column zero will be ignored.

    Returns:
      A list of absolute module names imported by the given source.

    >>> modulename = 'foo.xxx'
    >>> localmods = {'foo.__init__': True,
    ...              'foo.foo1': True, 'foo.foo2': True,
    ...              'foo.bar.__init__': True, 'foo.bar.bar1': True,
    ...              'baz.__init__': True, 'baz.baz1': True }
    >>> # standard library (= not locally defined ones)
    >>> sorted(imported_modules(
    ...        'from stdlib1 import foo, bar; import stdlib2',
    ...        modulename, localmods))
    []
    >>> # relative importing
    >>> sorted(imported_modules(
    ...        'import foo1; from bar import bar1',
    ...        modulename, localmods))
    ['foo.bar.__init__', 'foo.bar.bar1', 'foo.foo1']
    >>> sorted(imported_modules(
    ...        'from bar.bar1 import name1, name2, name3',
    ...        modulename, localmods))
    ['foo.bar.bar1']
    >>> # absolute importing
    >>> sorted(imported_modules(
    ...        'from baz import baz1, name1',
    ...        modulename, localmods))
    ['baz.__init__', 'baz.baz1']
    >>> # mixed importing, even though it shouldn't be recommended
    >>> sorted(imported_modules(
    ...        'import stdlib, foo1, baz',
    ...        modulename, localmods))
    ['baz.__init__', 'foo.foo1']
    >>> # ignore_nested
    >>> sorted(imported_modules(
    ... '''import foo
    ... def wat():
    ...     import bar
    ... ''', modulename, localmods))
    ['foo.__init__', 'foo.bar.__init__']
    >>> sorted(imported_modules(
    ... '''import foo
    ... def wat():
    ...     import bar
    ... ''', modulename, localmods, ignore_nested=True))
    ['foo.__init__']
    """
    fromlocal = fromlocalfunc(modulename, localmods)
    for node in ast.walk(ast.parse(source)):
        if ignore_nested and getattr(node, 'col_offset', 0) > 0:
            continue
        if isinstance(node, ast.Import):
            for n in node.names:
                found = fromlocal(n.name)
                if not found:
                    # this should import standard library
                    continue
                yield found[1]
        elif isinstance(node, ast.ImportFrom):
            found = fromlocal(node.module, node.level)
            if not found:
                # this should import standard library
                continue

            absname, dottedpath, hassubmod = found
            yield dottedpath
            if not hassubmod:
                # examination of "node.names" should be redundant
                # e.g.: from mercurial.node import nullid, nullrev
                continue

            prefix = absname + '.'
            for n in node.names:
                found = fromlocal(prefix + n.name)
                if not found:
                    # this should be a function or a property of "node.module"
                    continue
                yield found[1]

def verify_stdlib_on_own_line(source):
    """Given some python source, verify that stdlib imports are done
    in separate statements from relative local module imports.

    Observing this limitation is important as it works around an
    annoying lib2to3 bug in relative import rewrites:
    http://bugs.python.org/issue19510.

    >>> list(verify_stdlib_on_own_line('import sys, foo'))
    ['mixed imports\\n   stdlib:    sys\\n   relative:  foo']
    >>> list(verify_stdlib_on_own_line('import sys, os'))
    []
    >>> list(verify_stdlib_on_own_line('import foo, bar'))
    []
    """
    for node in ast.walk(ast.parse(source)):
        if isinstance(node, ast.Import):
            from_stdlib = {False: [], True: []}
            for n in node.names:
                from_stdlib[n.name in stdlib_modules].append(n.name)
            if from_stdlib[True] and from_stdlib[False]:
                yield ('mixed imports\n   stdlib:    %s\n   relative:  %s' %
                       (', '.join(sorted(from_stdlib[True])),
                        ', '.join(sorted(from_stdlib[False]))))

class CircularImport(Exception):
    pass

def checkmod(mod, imports):
    shortest = {}
    visit = [[mod]]
    while visit:
        path = visit.pop(0)
        for i in sorted(imports.get(path[-1], [])):
            if len(path) < shortest.get(i, 1000):
                shortest[i] = len(path)
                if i in path:
                    if i == path[0]:
                        raise CircularImport(path)
                    continue
                visit.append(path + [i])

def rotatecycle(cycle):
    """arrange a cycle so that the lexicographically first module listed first

    >>> rotatecycle(['foo', 'bar'])
    ['bar', 'foo', 'bar']
    """
    lowest = min(cycle)
    idx = cycle.index(lowest)
    return cycle[idx:] + cycle[:idx] + [lowest]

def find_cycles(imports):
    """Find cycles in an already-loaded import graph.

    All module names recorded in `imports` should be absolute one.

    >>> imports = {'top.foo': ['top.bar', 'os.path', 'top.qux'],
    ...            'top.bar': ['top.baz', 'sys'],
    ...            'top.baz': ['top.foo'],
    ...            'top.qux': ['top.foo']}
    >>> print '\\n'.join(sorted(find_cycles(imports)))
    top.bar -> top.baz -> top.foo -> top.bar
    top.foo -> top.qux -> top.foo
    """
    cycles = set()
    for mod in sorted(imports.iterkeys()):
        try:
            checkmod(mod, imports)
        except CircularImport as e:
            cycle = e.args[0]
            cycles.add(" -> ".join(rotatecycle(cycle)))
    return cycles

def _cycle_sortkey(c):
    return len(c), c

def main(argv):
    if len(argv) < 2 or (argv[1] == '-' and len(argv) > 2):
        print 'Usage: %s {-|file [file] [file] ...}'
        return 1
    if argv[1] == '-':
        argv = argv[:1]
        argv.extend(l.rstrip() for l in sys.stdin.readlines())
    localmods = {}
    used_imports = {}
    any_errors = False
    for source_path in argv[1:]:
        modname = dotted_name_of_path(source_path, trimpure=True)
        localmods[modname] = source_path
    for modname, source_path in sorted(localmods.iteritems()):
        f = open(source_path)
        src = f.read()
        used_imports[modname] = sorted(
            imported_modules(src, modname, localmods, ignore_nested=True))
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
