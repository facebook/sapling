# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# extensions.py - extension handling for mercurial
#
# Copyright 2005-2007 Olivia Mackall <olivia@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import errno
import functools
import importlib
import inspect
import os
import sys

from . import cmdutil, configitems, error, pycompat, util
from .i18n import _, gettext


_preimported = {}
_extensions = {}
_disabledextensions = {}
_aftercallbacks = {}
_order = []

# These extensions are never imported, even if the user tells us to.
#
# (If you permanently sunset an extension, add it here.)
_ignoreextensions = {
    "backups",
    "bookmarks",
    "bundle2hooks",
    "censor",
    "children",
    "cleanobsstore",
    "clindex",
    "color",
    "configwarn",
    "convert",
    "directaccess",
    "eden",
    "eol",
    "factotum",
    "fastmanifest",
    "fastpartialmatch",
    "fbsparse",
    "fixcorrupt",
    "graphlog",
    "gitlookup",
    "hbisect",
    "hgcia",
    "hgk",
    "hiddenerror",
    "inotify",
    "interhg",
    "morecolors",
    "mq",
    "nointerrupt",
    "perftweaks",
    "purge",
    "obsshelve",
    "patchrmdir",
    "parentrevspec",
    "progress",
    "releasenotes",
    "relink",
    "remoteid",
    "stat",
    "strip",
    "treedirstate",
    "uncommit",
    "upgradegeneraldelta",
    "win32text",
}
_exclude_list = {"extlib"}


# root of the directory, or installed distribution
_hgroot = os.path.abspath(os.path.join(__file__, "../../"))
_sysroot = os.path.abspath(os.path.join(os.__file__, "../"))

# List of extensions to always enable by default, unless overwritten by config.
#
# This allows us to integrate extensions into the codebase while leaving them in
# ext/ -- useful for extensions that need cleaning up, or significant
# integration work, to be brought into mercurial/.
DEFAULT_EXTENSIONS = {
    "conflictinfo",
    "debugshell",
    "errorredirect",
    "githelp",
    "lz4revlog",
    "mergedriver",
    "progressfile",
    "sampling",
    "remotefilelog",
}

# Similar to DEFAULT_EXTENSIONS. But cannot be disabled.
ALWAYS_ON_EXTENSIONS = ()


def isenabled(ui, name):
    for format in ["%s", "ext.%s"]:
        conf = ui.config("extensions", format % name)
        if name in ALWAYS_ON_EXTENSIONS:
            return True
        if conf is not None and not conf.startswith("!"):
            return True
        # Check DEFAULT_EXTENSIONS if no config for this extension was
        # specified.
        if conf is None and name in DEFAULT_EXTENSIONS:
            return True


def extensions(ui=None):
    if ui:
        enabled = lambda name: isenabled(ui, name)
    else:
        enabled = lambda name: True
    for name in _order:
        module = _extensions[name]
        if module and enabled(name):
            yield name, module


def find(name):
    """return module with given extension name"""
    mod = None
    try:
        mod = _extensions[name]
    except KeyError:
        for k, v in pycompat.iteritems(_extensions):
            if k.endswith("." + name) or k.endswith("/" + name):
                mod = v
                break
    if not mod:
        raise KeyError(name)
    return mod


def isloaded(name):
    """test if an extension is loaded

    Unlike isenabled, returns false if an extension is enabled in config but
    fails to load (ex. when importing the extension an exception happened).
    """
    return name in _extensions


def loadpath(path, module_name):
    """loads the given extension from the given path

    Note, this cannot be used to load core extensions, since the relative
    imports they use no longer work within loadpath.
    """
    module_name = module_name.replace(".", "_")
    path = util.normpath(util.expandpath(path))
    # TODO: check whether path is "trusted" or not
    if ":" in path:
        prefix, content = path.split(":", 1)
        if prefix == "python-base64":
            import base64

            source = base64.b64decode(content)
            return loadsource(source, module_name)

    try:
        if os.path.isdir(path):
            path = os.path.join(path, "__init__.py")

        if not os.path.exists(path):
            raise IOError(errno.ENOENT, "No such file or directory", path)

        spec = importlib.util.spec_from_file_location(module_name, path)
        module = importlib.util.module_from_spec(spec)
        sys.modules[module_name] = module
        spec.loader.exec_module(module)
        return module
    except IOError as exc:
        if not exc.filename:
            exc.filename = path  # python does not fill this
        raise


def loadsource(source, name):
    """make a Python module from provided Python source code"""
    # See load_source_module in Python/import.c for how this should work.
    code = compile(source, "<%s>" % name, "exec")
    # Get the module constructor. Note: 'sys' might be a demandimport proxy.
    # Get the real 'sys' module by using sys.modules.
    modtype = type(sys.modules["sys"])
    mod = modtype(name)
    env = mod.__dict__
    exec(code, env, env)
    mod.__file__ = f"<{name}>"
    return mod


def preimport(name):
    """preimport an ext module"""
    mod = getattr(__import__("edenscm.ext.%s" % name).ext, name)
    # use a dict explicitly - "dict.get" is much faster than "import" again.
    _preimported[name] = mod


def loaddefault(name, reportfunc=None):
    """load extensions without a specified path"""
    mod = _preimported.get(name)
    if mod:
        return mod
    mod = _importh("edenscm.ext.%s" % name)
    return mod


_collectedimports = []  # [(name, path)]


def _collectimport(orig, name, *args, **kwargs):
    """collect imports to _collectedimports"""
    mod = orig(name, *args, **kwargs)
    fromlist = args[2] if len(args) >= 3 else None
    try:
        # If the fromlist argument is non-empty then __import__ returns the exact
        # module requested.  Otherwise it returns the top-level module in the hierarchy
        # and we need to resolve it with _resolvenestedmodules()
        if fromlist:
            nestedmod = mod
        else:
            nestedmod = _resolvenestedmodules(mod, name)
        path = os.path.abspath(inspect.getfile(nestedmod))
        _collectedimports.append((name, path))
    except Exception:
        pass
    return mod


def _importh(name):
    """import and return the <name> module"""
    mod = __import__(name)
    return _resolvenestedmodules(mod, name)


def _resolvenestedmodules(mod, name):
    """resolve nested modules

    __import__('x.y.z') returns module x when no fromlist is specified.
    This function resolves it and return the module "z".
    """
    components = name.split(".")
    for comp in components[1:]:
        mod = getattr(mod, comp)
    return mod


def _importext(name, path=None, reportfunc=None):
    if path:
        # the module will be loaded in sys.modules
        # choose an unique name so that it doesn't
        # conflicts with other modules
        mod = loadpath(path, "edenscm.ext.%s" % name)
    else:
        mod = loaddefault(name, reportfunc)
    return mod


def _reportimporterror(ui, err, failed, next):
    # note: this ui.debug happens before --debug is processed,
    #       Use --config ui.debug=1 to see them.
    ui.debug(
        "could not import %s (%s): trying %s\n" % (failed, util.forcebytestr(err), next)
    )
    if ui.debugflag:
        ui.traceback()


# attributes set by registrar.command
_cmdfuncattrs = ("norepo", "optionalrepo", "inferrepo")


def _validatecmdtable(ui, cmdtable):
    """Check if extension commands have required attributes"""
    for c, e in pycompat.iteritems(cmdtable):
        f = e[0]
        if getattr(f, "_deprecatedregistrar", False):
            ui.deprecwarn(
                "cmdutil.command is deprecated, use "
                "registrar.command to register '%s'" % c,
                "4.6",
            )
        missing = [a for a in _cmdfuncattrs if not util.safehasattr(f, a)]
        if not missing:
            for option in e[1]:
                default = option[2]
                if (type(b"") != type("")) and isinstance(default, type(b"")):
                    # TODO: write a test after Python 3 migration
                    raise error.ProgrammingError(
                        "option '%s.%s' has a bytes default value" % (c, option[1]),
                        hint=(
                            "change the %s.%s default value to a string"
                            % (c, option[1])
                        ),
                    )
            continue
        raise error.ProgrammingError(
            "missing attributes: %s" % ", ".join(missing),
            hint="use @command decorator to register '%s'" % c,
        )


_warned = set()


def load(ui, name, path):
    if name.startswith("ext.") or name.startswith("ext/"):
        shortname = name[4:]
        if name not in _warned:
            _warned.add(name)
            msg = _("'ext' prefix in [extensions] config section is deprecated.\n")
            location = ui.configsource("extensions", name)
            if location and ":" in location:
                msg += _("(hint: replace %r with %r at %s)\n") % (
                    name,
                    shortname,
                    location,
                )
            else:
                msg += _("(hint: replace %r with %r)\n") % (name, shortname)
            ui.warn(msg)
    else:
        shortname = name
    if shortname in _ignoreextensions:
        return None
    if shortname in _extensions:
        return _extensions[shortname]
    _extensions[shortname] = None
    # If the entry point is not 'hg', the code was executed in a non-standard
    # way and we cannot assume the filesystem layout. Be permissive to avoid
    # false positives.
    from . import dispatch  # noqa: F401; avoid cycles

    mod = _importext(shortname, path, bind(_reportimporterror, ui))

    # "mercurial" and "ext" were moved. Detect wrong module imports.
    if ui.configbool("devel", "all-warnings"):
        if "ext" in sys.modules:
            ui.develwarn("extension %s imported incorrect modules" % name)

    # Before we do anything with the extension, check against minimum stated
    # compatibility. This gives extension authors a mechanism to have their
    # extensions short circuit when loaded with a known incompatible version
    # of Mercurial.
    minver = getattr(mod, "minimumhgversion", None)
    if minver and util.versiontuple(minver, 2) > util.versiontuple(n=2):
        ui.warn(
            _(
                "(third party extension %s requires version %s or newer "
                "of Mercurial; disabling)\n"
            )
            % (shortname, minver)
        )
        return
    _validatecmdtable(ui, getattr(mod, "cmdtable", {}))

    _extensions[shortname] = mod
    _order.append(shortname)
    for fn in _aftercallbacks.get(shortname, []):
        fn(loaded=True)
    return mod


def _runuisetup(name, ui):
    uisetup = getattr(_extensions[name], "uisetup", None)
    if uisetup:
        try:
            uisetup(ui)
        except Exception as inst:
            ui.traceback(force=True)
            msg = util.forcebytestr(inst)
            ui.warn(
                _("failed to set up extension %s: %s\n") % (name, msg),
                notice=_("warning"),
            )
            return False
    return True


def _runextsetup(name, ui):
    extsetup = getattr(_extensions[name], "extsetup", None)
    if extsetup:
        try:
            try:
                extsetup(ui)
            except TypeError:
                # Try to use getfullargspec (Python 3) first, and fall
                # back to getargspec only if it doesn't exist so as to
                # avoid warnings.
                if getattr(inspect, "getfullargspec", getattr(inspect, "getargspec"))(
                    extsetup
                ).args:
                    raise
                extsetup()  # old extsetup with no ui argument
        except Exception as inst:
            ui.traceback(force=True)
            msg = util.forcebytestr(inst)
            ui.warn(
                _("failed to set up extension %s: %s\n") % (name, msg),
                notice=_("warning"),
            )
            return False
    return True


def loadall(ui, include_list=None):
    result = ui.configitems("extensions")
    resultkeys = set([name for name, loc in result])

    # Add all extensions in `DEFAULT_EXTENSIONS` that were not defined by
    # extensions.
    result += [
        (name, "") for name in sorted(DEFAULT_EXTENSIONS) if name not in resultkeys
    ]

    # Always enable `ALWAYS_ON_EXTENSIONS`
    result = [(k, v) for (k, v) in result if k not in ALWAYS_ON_EXTENSIONS]
    result += [(name, "") for name in sorted(ALWAYS_ON_EXTENSIONS)]

    if include_list is not None:
        result = [(k, v) for (k, v) in result if k in include_list]

    alreadyenabled = set(_order)
    for (name, path) in result:
        if path:
            if path[0:1] == "!":
                _disabledextensions[name] = path[1:]
                continue
        try:
            load(ui, name, path)
        except error.ForeignImportError:
            raise
        except Exception as inst:
            msg = util.forcebytestr(inst)
            if path:
                ui.warn(
                    _(
                        "extension %s is disabled because it cannot be imported from %s: %s\n"
                    )
                    % (name, path, msg),
                    notice=_("warning"),
                )
            else:
                ui.warn(
                    _("extension %s is disabled because it cannot be imported: %s\n")
                    % (name, msg),
                    notice=_("warning"),
                )
            if isinstance(inst, error.Hint) and inst.hint:
                ui.warn(_("(%s)\n") % inst.hint)
            ui.traceback()

    newextensions = list(name for name in _order if name not in alreadyenabled)

    # list of (objname, loadermod, loadername) tuple:
    # - objname is the name of an object in extension module,
    #   from which extra information is loaded
    # - loadermod is the module where loader is placed
    # - loadername is the name of the function,
    #   which takes (ui, extensionname, extraobj) arguments
    #
    # This one is for the list of item that must be run before running any setup
    earlyextraloaders = [("configtable", configitems, "loadconfigtable")]
    _loadextra(ui, newextensions, earlyextraloaders)

    broken = set()
    for name in newextensions:
        if not _runuisetup(name, ui):
            broken.add(name)

    # Recompute since the order may have changed
    newextensions = list(name for name in _order if name not in alreadyenabled)

    for name in newextensions:
        if name in broken:
            continue
        if not _runextsetup(name, ui):
            broken.add(name)

    for name in broken:
        _extensions[name] = None

    # Call aftercallbacks that were never met.
    for shortname in _aftercallbacks:
        if shortname in _extensions:
            continue

        for fn in _aftercallbacks[shortname]:
            fn(loaded=False)

    # loadall() is called multiple times and lingering _aftercallbacks
    # entries could result in double execution. See issue4646.
    _aftercallbacks.clear()

    # delay importing avoids cyclic dependency (especially commands)
    from . import (
        autopull,
        color,
        commands,
        filemerge,
        fileset,
        hintutil,
        namespaces,
        revset,
        templatefilters,
        templatekw,
        templater,
    )

    # list of (objname, loadermod, loadername) tuple:
    # - objname is the name of an object in extension module,
    #   from which extra information is loaded
    # - loadermod is the module where loader is placed
    # - loadername is the name of the function,
    #   which takes (ui, extensionname, extraobj) arguments
    extraloaders = [
        ("autopullpredicate", autopull, "loadpredicate"),
        ("cmdtable", commands, "loadcmdtable"),
        ("colortable", color, "loadcolortable"),
        ("filesetpredicate", fileset, "loadpredicate"),
        ("hint", hintutil, "loadhint"),
        ("internalmerge", filemerge, "loadinternalmerge"),
        ("namespacepredicate", namespaces, "loadpredicate"),
        ("revsetpredicate", revset, "loadpredicate"),
        ("templatefilter", templatefilters, "loadfilter"),
        ("templatefunc", templater, "loadfunction"),
        ("templatekeyword", templatekw, "loadkeyword"),
    ]
    _loadextra(ui, newextensions, extraloaders)


def _loadextra(ui, newextensions, extraloaders):
    for name in newextensions:
        module = _extensions[name]
        if not module:
            continue  # loading this module failed

        for objname, loadermod, loadername in extraloaders:
            extraobj = getattr(module, objname, None)
            if extraobj is not None:
                getattr(loadermod, loadername)(ui, name, extraobj)


def afterloaded(extension, callback):
    """Run the specified function after a named extension is loaded.

    If the named extension is already loaded, the callback will be called
    immediately.

    If the named extension never loads, the callback will be called after
    all extensions have been loaded.

    The callback receives the named argument ``loaded``, which is a boolean
    indicating whether the dependent extension actually loaded.
    """

    if extension in _extensions:
        # Report loaded as False if the extension is disabled
        loaded = _extensions[extension] is not None
        callback(loaded=loaded)
    else:
        _aftercallbacks.setdefault(extension, []).append(callback)


def bind(func, *args):
    """Partial function application

    Returns a new function that is the partial application of args and kwargs
    to func.  For example,

        f(1, 2, bar=3) === bind(f, 1)(2, bar=3)"""
    assert callable(func)

    def closure(*a, **kw):
        return func(*(args + a), **kw)

    return closure


def _updatewrapper(wrap, origfn, unboundwrapper):
    """Copy and add some useful attributes to wrapper"""
    try:
        wrap.__name__ = origfn.__name__
    except AttributeError:
        pass
    wrap.__module__ = getattr(origfn, "__module__")
    wrap.__doc__ = getattr(origfn, "__doc__")
    wrap.__dict__.update(getattr(origfn, "__dict__", {}))
    wrap._origfunc = origfn
    wrap._unboundwrapper = unboundwrapper


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
    for alias, e in pycompat.iteritems(table):
        if e is entry:
            key = alias
            break

    origfn = entry[0]
    wrap = functools.partial(util.checksignature(wrapper), util.checksignature(origfn))
    _updatewrapper(wrap, origfn, wrapper)
    if docstring is not None:
        wrap.__doc__ += docstring

    newentry = list(entry)
    newentry[0] = wrap
    if synopsis is not None:
        newentry[2] += synopsis
    table[key] = tuple(newentry)
    return entry


def wrapfilecache(cls, propname, wrapper):
    """Wraps a filecache property.

    These can't be wrapped using the normal wrapfunction.
    """
    propname = propname
    assert callable(wrapper)
    for currcls in cls.__mro__:
        if propname in currcls.__dict__:
            origfn = currcls.__dict__[propname].func
            assert callable(origfn)

            def wrap(*args, **kwargs):
                return wrapper(origfn, *args, **kwargs)

            currcls.__dict__[propname].func = wrap
            break

    if currcls is object:
        raise AttributeError(r"type '%s' has no property '%s'" % (cls, propname))


class wrappedfunction(object):
    """context manager for temporarily wrapping a function"""

    def __init__(self, container, funcname, wrapper):
        assert callable(wrapper)
        self._container = container
        self._funcname = funcname
        self._wrapper = wrapper

    def __enter__(self):
        wrapfunction(self._container, self._funcname, self._wrapper)

    def __exit__(self, exctype, excvalue, traceback):
        unwrapfunction(self._container, self._funcname, self._wrapper)


def wrapfunction(container, funcname, wrapper):
    """Wrap the function named funcname in container

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
    """
    assert callable(wrapper)

    origfn = getattr(container, funcname)
    assert callable(origfn)
    if inspect.ismodule(container):
        # origfn is not an instance or class method. "partial" can be used.
        # "partial" won't insert a frame in traceback.
        wrap = functools.partial(wrapper, origfn)
    else:
        # "partial" cannot be safely used. Emulate its effect by using "bind".
        # The downside is one more frame in traceback.
        wrap = bind(wrapper, origfn)
    _updatewrapper(wrap, origfn, wrapper)
    setattr(container, funcname, wrap)
    return origfn


def unwrapfunction(container, funcname, wrapper=None):
    """undo wrapfunction

    If wrappers is None, undo the last wrap. Otherwise removes the wrapper
    from the chain of wrappers.

    Return the removed wrapper.
    Raise IndexError if wrapper is None and nothing to unwrap; ValueError if
    wrapper is not None but is not found in the wrapper chain.
    """
    chain = getwrapperchain(container, funcname)
    origfn = chain.pop()
    if wrapper is None:
        wrapper = chain[0]
    chain.remove(wrapper)
    setattr(container, funcname, origfn)
    for w in reversed(chain):
        wrapfunction(container, funcname, w)
    return wrapper


def getwrapperchain(container, funcname):
    """get a chain of wrappers of a function

    Return a list of functions: [newest wrapper, ..., oldest wrapper, origfunc]

    The wrapper functions are the ones passed to wrapfunction, whose first
    argument is origfunc.
    """
    result = []
    fn = getattr(container, funcname)
    while fn:
        assert callable(fn)
        result.append(getattr(fn, "_unboundwrapper", fn))
        fn = getattr(fn, "_origfunc", None)
    return result


def _disabledpaths(strip_init=False):
    """find paths of disabled extensions. returns a dict of {name: path}
    removes /__init__.py from packages if strip_init is True"""
    from edenscm import ext

    extpath = os.path.dirname(os.path.abspath(ext.__file__))
    try:  # might not be a filesystem path
        files = os.listdir(extpath)
    except OSError:
        return {}

    exts = {}
    for e in files:
        if e.endswith(".py"):
            name = e.rsplit(".", 1)[0]
            path = os.path.join(extpath, e)
        else:
            name = e
            path = os.path.join(extpath, e, "__init__.py")
            if not os.path.exists(path):
                continue
            if strip_init:
                path = os.path.dirname(path)
        if name in exts or name in _order or name == "__init__":
            continue
        exts[name] = path
    for name, path in pycompat.iteritems(_disabledextensions):
        # If no path was provided for a disabled extension (e.g. "color=!"),
        # don't replace the path we already found by the scan above.
        if path:
            exts[name] = path
    return exts


def _moduledoc(file):
    """return the top-level python documentation for the given file

    Loosely inspired by pydoc.source_synopsis(), but rewritten to
    handle triple quotes and to return the whole text instead of just
    the synopsis"""
    result = []

    line = file.readline()
    while line[:1] == "#" or not line.strip():
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
                return None  # unmatched delimiter
            result.append(line)
            line = file.readline()
    else:
        return None

    return "".join(result)


def _disabledhelp(path):
    """retrieve help synopsis of a disabled extension (without importing)"""
    try:
        file = open(path)
    except IOError:
        return
    else:
        doc = _moduledoc(file)
        file.close()

    if doc:  # extracting localized synopsis
        return gettext(doc)
    else:
        return _("(no help text available)")


def disabled():
    """find disabled extensions from ext. returns a dict of {name: desc}"""
    try:
        # pyre-fixme[21]: Could not find name `__index__` in `edenscm.ext`.
        from edenscm.ext import __index__

        return dict(
            (name, gettext(desc))
            for name, desc in pycompat.iteritems(__index__.docs)
            if name not in _order and name not in _exclude_list
        )
    except (ImportError, AttributeError):
        pass

    paths = _disabledpaths()
    if not paths:
        return {}

    exts = {}
    for name, path in pycompat.iteritems(paths):
        doc = _disabledhelp(path)
        if doc and name not in _exclude_list:
            exts[name] = doc.splitlines()[0]

    return exts


def disabledext(name):
    """find a specific disabled extension from ext. returns desc"""
    try:
        from edenscm.ext import __index__

        if name in _order:  # enabled
            return
        elif name in _exclude_list:
            return
        else:
            return gettext(__index__.docs.get(name))
    except (ImportError, AttributeError):
        pass

    paths = _disabledpaths()
    if name in paths and name not in _exclude_list:
        return _disabledhelp(paths[name])


def disabledcmd(ui, cmd):
    """import disabled extensions until cmd is found.
    returns (cmdname, extname, module)"""

    paths = _disabledpaths(strip_init=True)
    if not paths:
        raise error.UnknownCommand(cmd)

    def findcmd(cmd, name, path):
        try:
            mod = loadpath(path, "ext.%s" % name)
        except Exception:
            return
        try:
            aliases, entry = cmdutil.findcmd(cmd, getattr(mod, "cmdtable", {}))
        except (error.AmbiguousCommand, error.UnknownCommand):
            return
        except Exception:
            ui.warn(_("error finding commands in %s\n") % path, notice=_("warning"))
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
        for name, path in pycompat.iteritems(paths):
            ext = findcmd(cmd, name, path)
            if ext:
                break
    if ext and "DEPRECATED" not in ext.__doc__:
        return ext

    raise error.UnknownCommand(cmd)


def enabled(shortname=True):
    """return a dict of {name: desc} of extensions"""
    exts = {}
    for ename, ext in extensions():
        doc = gettext(ext.__doc__) or _("(no help text available)")
        if shortname:
            ename = ename.split(".")[-1]
        exts[ename] = doc.splitlines()[0].strip()

    return exts


def notloaded():
    """return short names of extensions that failed to load"""
    return [name for name, mod in pycompat.iteritems(_extensions) if mod is None]


def moduleversion(module):
    """return version information from given module as a string"""
    if util.safehasattr(module, "getversion") and callable(module.getversion):
        version = module.getversion()
    elif util.safehasattr(module, "__version__"):
        version = module.__version__
    else:
        version = ""
    if isinstance(version, (list, tuple)):
        version = ".".join(str(o) for o in version)
    return version


def ismoduleinternal(module):
    exttestedwith = getattr(module, "testedwith", None)
    return exttestedwith == "ships-with-hg-core"
