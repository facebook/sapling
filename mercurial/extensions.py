# extensions.py - extension handling for mercurial
#
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import imp
import os

from .i18n import (
    _,
    gettext,
)

from . import (
    cmdutil,
    error,
    util,
)

_extensions = {}
_aftercallbacks = {}
_order = []
_builtin = set(['hbisect', 'bookmarks', 'parentrevspec', 'progress', 'interhg',
                'inotify'])

def extensions(ui=None):
    if ui:
        def enabled(name):
            for format in ['%s', 'hgext.%s']:
                conf = ui.config('extensions', format % name)
                if conf is not None and not conf.startswith('!'):
                    return True
    else:
        enabled = lambda name: True
    for name in _order:
        module = _extensions[name]
        if module and enabled(name):
            yield name, module

def find(name):
    '''return module with given extension name'''
    mod = None
    try:
        mod = _extensions[name]
    except KeyError:
        for k, v in _extensions.iteritems():
            if k.endswith('.' + name) or k.endswith('/' + name):
                mod = v
                break
    if not mod:
        raise KeyError(name)
    return mod

def loadpath(path, module_name):
    module_name = module_name.replace('.', '_')
    path = util.normpath(util.expandpath(path))
    if os.path.isdir(path):
        # module/__init__.py style
        d, f = os.path.split(path)
        fd, fpath, desc = imp.find_module(f, [d])
        return imp.load_module(module_name, fd, fpath, desc)
    else:
        try:
            return imp.load_source(module_name, path)
        except IOError as exc:
            if not exc.filename:
                exc.filename = path # python does not fill this
            raise

def _importh(name):
    """import and return the <name> module"""
    mod = __import__(name)
    components = name.split('.')
    for comp in components[1:]:
        mod = getattr(mod, comp)
    return mod

def _reportimporterror(ui, err, failed, next):
    ui.debug('could not import %s (%s): trying %s\n'
             % (failed, err, next))
    if ui.debugflag:
        ui.traceback()

def load(ui, name, path):
    if name.startswith('hgext.') or name.startswith('hgext/'):
        shortname = name[6:]
    else:
        shortname = name
    if shortname in _builtin:
        return None
    if shortname in _extensions:
        return _extensions[shortname]
    _extensions[shortname] = None
    if path:
        # the module will be loaded in sys.modules
        # choose an unique name so that it doesn't
        # conflicts with other modules
        mod = loadpath(path, 'hgext.%s' % name)
    else:
        try:
            mod = _importh("hgext.%s" % name)
        except ImportError as err:
            _reportimporterror(ui, err, "hgext.%s" % name, name)
            try:
                mod = _importh("hgext3rd.%s" % name)
            except ImportError as err:
                _reportimporterror(ui, err, "hgext3rd.%s" % name, name)
                mod = _importh(name)

    # Before we do anything with the extension, check against minimum stated
    # compatibility. This gives extension authors a mechanism to have their
    # extensions short circuit when loaded with a known incompatible version
    # of Mercurial.
    minver = getattr(mod, 'minimumhgversion', None)
    if minver and util.versiontuple(minver, 2) > util.versiontuple(n=2):
        ui.warn(_('(third party extension %s requires version %s or newer '
                  'of Mercurial; disabling)\n') % (shortname, minver))
        return

    _extensions[shortname] = mod
    _order.append(shortname)
    for fn in _aftercallbacks.get(shortname, []):
        fn(loaded=True)
    return mod

def loadall(ui):
    result = ui.configitems("extensions")
    newindex = len(_order)
    for (name, path) in result:
        if path:
            if path[0] == '!':
                continue
        try:
            load(ui, name, path)
        except KeyboardInterrupt:
            raise
        except Exception as inst:
            if path:
                ui.warn(_("*** failed to import extension %s from %s: %s\n")
                        % (name, path, inst))
            else:
                ui.warn(_("*** failed to import extension %s: %s\n")
                        % (name, inst))
            ui.traceback()

    for name in _order[newindex:]:
        uisetup = getattr(_extensions[name], 'uisetup', None)
        if uisetup:
            uisetup(ui)

    for name in _order[newindex:]:
        extsetup = getattr(_extensions[name], 'extsetup', None)
        if extsetup:
            try:
                extsetup(ui)
            except TypeError:
                if extsetup.func_code.co_argcount != 0:
                    raise
                extsetup() # old extsetup with no ui argument

    # Call aftercallbacks that were never met.
    for shortname in _aftercallbacks:
        if shortname in _extensions:
            continue

        for fn in _aftercallbacks[shortname]:
            fn(loaded=False)

    # loadall() is called multiple times and lingering _aftercallbacks
    # entries could result in double execution. See issue4646.
    _aftercallbacks.clear()

def afterloaded(extension, callback):
    '''Run the specified function after a named extension is loaded.

    If the named extension is already loaded, the callback will be called
    immediately.

    If the named extension never loads, the callback will be called after
    all extensions have been loaded.

    The callback receives the named argument ``loaded``, which is a boolean
    indicating whether the dependent extension actually loaded.
    '''

    if extension in _extensions:
        callback(loaded=True)
    else:
        _aftercallbacks.setdefault(extension, []).append(callback)

def bind(func, *args):
    '''Partial function application

      Returns a new function that is the partial application of args and kwargs
      to func.  For example,

          f(1, 2, bar=3) === bind(f, 1)(2, bar=3)'''
    assert callable(func)
    def closure(*a, **kw):
        return func(*(args + a), **kw)
    return closure

def _updatewrapper(wrap, origfn):
    '''Copy attributes to wrapper function'''
    wrap.__module__ = getattr(origfn, '__module__')
    wrap.__doc__ = getattr(origfn, '__doc__')
    wrap.__dict__.update(getattr(origfn, '__dict__', {}))

def wrapcommand(table, command, wrapper, synopsis=None, docstring=None):
    '''Wrap the command named `command' in table

    Replace command in the command table with wrapper. The wrapped command will
    be inserted into the command table specified by the table argument.

    The wrapper will be called like

      wrapper(orig, *args, **kwargs)

    where orig is the original (wrapped) function, and *args, **kwargs
    are the arguments passed to it.

    Optionally append to the command synopsis and docstring, used for help.
    For example, if your extension wraps the ``bookmarks`` command to add the
    flags ``--remote`` and ``--all`` you might call this function like so:

      synopsis = ' [-a] [--remote]'
      docstring = """

      The ``remotenames`` extension adds the ``--remote`` and ``--all`` (``-a``)
      flags to the bookmarks command. Either flag will show the remote bookmarks
      known to the repository; ``--remote`` will also suppress the output of the
      local bookmarks.
      """

      extensions.wrapcommand(commands.table, 'bookmarks', exbookmarks,
                             synopsis, docstring)
    '''
    assert callable(wrapper)
    aliases, entry = cmdutil.findcmd(command, table)
    for alias, e in table.iteritems():
        if e is entry:
            key = alias
            break

    origfn = entry[0]
    wrap = bind(util.checksignature(wrapper), util.checksignature(origfn))
    _updatewrapper(wrap, origfn)
    if docstring is not None:
        wrap.__doc__ += docstring

    newentry = list(entry)
    newentry[0] = wrap
    if synopsis is not None:
        newentry[2] += synopsis
    table[key] = tuple(newentry)
    return entry

def wrapfunction(container, funcname, wrapper):
    '''Wrap the function named funcname in container

    Replace the funcname member in the given container with the specified
    wrapper. The container is typically a module, class, or instance.

    The wrapper will be called like

      wrapper(orig, *args, **kwargs)

    where orig is the original (wrapped) function, and *args, **kwargs
    are the arguments passed to it.

    Wrapping methods of the repository object is not recommended since
    it conflicts with extensions that extend the repository by
    subclassing. All extensions that need to extend methods of
    localrepository should use this subclassing trick: namely,
    reposetup() should look like

      def reposetup(ui, repo):
          class myrepo(repo.__class__):
              def whatever(self, *args, **kwargs):
                  [...extension stuff...]
                  super(myrepo, self).whatever(*args, **kwargs)
                  [...extension stuff...]

          repo.__class__ = myrepo

    In general, combining wrapfunction() with subclassing does not
    work. Since you cannot control what other extensions are loaded by
    your end users, you should play nicely with others by using the
    subclass trick.
    '''
    assert callable(wrapper)

    origfn = getattr(container, funcname)
    assert callable(origfn)
    wrap = bind(wrapper, origfn)
    _updatewrapper(wrap, origfn)
    setattr(container, funcname, wrap)
    return origfn

def _disabledpaths(strip_init=False):
    '''find paths of disabled extensions. returns a dict of {name: path}
    removes /__init__.py from packages if strip_init is True'''
    import hgext
    extpath = os.path.dirname(os.path.abspath(hgext.__file__))
    try: # might not be a filesystem path
        files = os.listdir(extpath)
    except OSError:
        return {}

    exts = {}
    for e in files:
        if e.endswith('.py'):
            name = e.rsplit('.', 1)[0]
            path = os.path.join(extpath, e)
        else:
            name = e
            path = os.path.join(extpath, e, '__init__.py')
            if not os.path.exists(path):
                continue
            if strip_init:
                path = os.path.dirname(path)
        if name in exts or name in _order or name == '__init__':
            continue
        exts[name] = path
    return exts

def _moduledoc(file):
    '''return the top-level python documentation for the given file

    Loosely inspired by pydoc.source_synopsis(), but rewritten to
    handle triple quotes and to return the whole text instead of just
    the synopsis'''
    result = []

    line = file.readline()
    while line[:1] == '#' or not line.strip():
        line = file.readline()
        if not line:
            break

    start = line[:3]
    if start == '"""' or start == "'''":
        line = line[3:]
        while line:
            if line.rstrip().endswith(start):
                line = line.split(start)[0]
                if line:
                    result.append(line)
                break
            elif not line:
                return None # unmatched delimiter
            result.append(line)
            line = file.readline()
    else:
        return None

    return ''.join(result)

def _disabledhelp(path):
    '''retrieve help synopsis of a disabled extension (without importing)'''
    try:
        file = open(path)
    except IOError:
        return
    else:
        doc = _moduledoc(file)
        file.close()

    if doc: # extracting localized synopsis
        return gettext(doc).splitlines()[0]
    else:
        return _('(no help text available)')

def disabled():
    '''find disabled extensions from hgext. returns a dict of {name: desc}'''
    try:
        from hgext import __index__
        return dict((name, gettext(desc))
                    for name, desc in __index__.docs.iteritems()
                    if name not in _order)
    except (ImportError, AttributeError):
        pass

    paths = _disabledpaths()
    if not paths:
        return {}

    exts = {}
    for name, path in paths.iteritems():
        doc = _disabledhelp(path)
        if doc:
            exts[name] = doc

    return exts

def disabledext(name):
    '''find a specific disabled extension from hgext. returns desc'''
    try:
        from hgext import __index__
        if name in _order:  # enabled
            return
        else:
            return gettext(__index__.docs.get(name))
    except (ImportError, AttributeError):
        pass

    paths = _disabledpaths()
    if name in paths:
        return _disabledhelp(paths[name])

def disabledcmd(ui, cmd, strict=False):
    '''import disabled extensions until cmd is found.
    returns (cmdname, extname, module)'''

    paths = _disabledpaths(strip_init=True)
    if not paths:
        raise error.UnknownCommand(cmd)

    def findcmd(cmd, name, path):
        try:
            mod = loadpath(path, 'hgext.%s' % name)
        except Exception:
            return
        try:
            aliases, entry = cmdutil.findcmd(cmd,
                getattr(mod, 'cmdtable', {}), strict)
        except (error.AmbiguousCommand, error.UnknownCommand):
            return
        except Exception:
            ui.warn(_('warning: error finding commands in %s\n') % path)
            ui.traceback()
            return
        for c in aliases:
            if c.startswith(cmd):
                cmd = c
                break
        else:
            cmd = aliases[0]
        return (cmd, name, mod)

    ext = None
    # first, search for an extension with the same name as the command
    path = paths.pop(cmd, None)
    if path:
        ext = findcmd(cmd, cmd, path)
    if not ext:
        # otherwise, interrogate each extension until there's a match
        for name, path in paths.iteritems():
            ext = findcmd(cmd, name, path)
            if ext:
                break
    if ext and 'DEPRECATED' not in ext.__doc__:
        return ext

    raise error.UnknownCommand(cmd)

def enabled(shortname=True):
    '''return a dict of {name: desc} of extensions'''
    exts = {}
    for ename, ext in extensions():
        doc = (gettext(ext.__doc__) or _('(no help text available)'))
        if shortname:
            ename = ename.split('.')[-1]
        exts[ename] = doc.splitlines()[0].strip()

    return exts

def notloaded():
    '''return short names of extensions that failed to load'''
    return [name for name, mod in _extensions.iteritems() if mod is None]

def moduleversion(module):
    '''return version information from given module as a string'''
    if (util.safehasattr(module, 'getversion')
          and callable(module.getversion)):
        version = module.getversion()
    elif util.safehasattr(module, '__version__'):
        version = module.__version__
    else:
        version = ''
    if isinstance(version, (list, tuple)):
        version = '.'.join(str(o) for o in version)
    return version

def ismoduleinternal(module):
    exttestedwith = getattr(module, 'testedwith', None)
    return exttestedwith == "internal"
