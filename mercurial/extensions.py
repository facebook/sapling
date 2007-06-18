# extensions.py - extension handling for mercurial
#
# Copyright 2005, 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import imp, os
import commands, hg, util, sys
from i18n import _

_extensions = {}

def find(name):
    '''return module with given extension name'''
    try:
        return _extensions[name]
    except KeyError:
        for k, v in _extensions.iteritems():
            if k.endswith('.' + name) or k.endswith('/' + name):
                return v
        raise KeyError(name)

def load(ui, name, path):
    if name in _extensions:
        return
    if path:
        # the module will be loaded in sys.modules
        # choose an unique name so that it doesn't
        # conflicts with other modules
        module_name = "hgext_%s" % name.replace('.', '_')
        if os.path.isdir(path):
            # module/__init__.py style
            d, f = os.path.split(path)
            fd, fpath, desc = imp.find_module(f, [d])
            mod = imp.load_module(module_name, fd, fpath, desc)
        else:
            mod = imp.load_source(module_name, path)
    else:
        def importh(name):
            mod = __import__(name)
            components = name.split('.')
            for comp in components[1:]:
                mod = getattr(mod, comp)
            return mod
        try:
            mod = importh("hgext.%s" % name)
        except ImportError:
            mod = importh(name)
    _extensions[name] = mod

    uisetup = getattr(mod, 'uisetup', None)
    if uisetup:
        uisetup(ui)
    reposetup = getattr(mod, 'reposetup', None)
    if reposetup:
        hg.repo_setup_hooks.append(reposetup)
    cmdtable = getattr(mod, 'cmdtable', {})
    overrides = [cmd for cmd in cmdtable if cmd in commands.table]
    if overrides:
        ui.warn(_("extension '%s' overrides commands: %s\n")
                % (name, " ".join(overrides)))
    commands.table.update(cmdtable)

def loadall(ui):
    result = ui.configitems("extensions")
    for i, (name, path) in enumerate(result):
        if path:
                path = os.path.expanduser(path)
        try:
            load(ui, name, path)
        except (util.SignalInterrupt, KeyboardInterrupt):
            raise
        except Exception, inst:
            ui.warn(_("*** failed to import extension %s: %s\n") %
                    (name, inst))
            if ui.print_exc():
                return 1

