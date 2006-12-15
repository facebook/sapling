# demandimport.py - global demand-loading of modules for Mercurial
#
# Copyright 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

'''
demandimport - automatic demandloading of modules

To enable this module, do:

  import demandimport; demandimport.enable()

Imports of the following forms will be demand-loaded:

  import a, b.c
  import a.b as c
  from a import b,c # a will be loaded immediately

These imports will not be delayed:

  from a import *
  b = __import__(a)
'''

_origimport = __import__

class _demandmod(object):
    """module demand-loader and proxy"""
    def __init__(self, name, globals, locals):
        if '.' in name:
            head, rest = name.split('.', 1)
            after = [rest]
        else:
            head = name
            after = []
        self.__dict__["_data"] = (head, globals, locals, after)
        self.__dict__["_module"] = None
    def _extend(self, name):
        """add to the list of submodules to load"""
        self._data[3].append(name)
    def _load(self):
        if not self._module:
            head, globals, locals, after = self._data
            mod = _origimport(head, globals, locals)
            # load submodules
            for x in after:
                hx = x
                if '.' in x:
                    hx = x.split('.')[0]
                if not hasattr(mod, hx):
                    setattr(mod, hx, _demandmod(x, mod.__dict__, mod.__dict__))
            # are we in the locals dictionary still?
            if locals and locals.get(head) == self:
                locals[head] = mod
            self.__dict__["_module"] = mod
    def __repr__(self):
        return "<unloaded module '%s'>" % self._data[0]
    def __call__(self, *args, **kwargs):
        raise TypeError("'unloaded module' object is not callable")
    def __getattr__(self, attr):
        self._load()
        return getattr(self._module, attr)
    def __setattr__(self, attr, val):
        self._load()
        setattr(self._module, attr, val)

def _demandimport(name, globals=None, locals=None, fromlist=None):
    if not locals or name in ignore or fromlist == ('*',):
        # these cases we can't really delay
        return _origimport(name, globals, locals, fromlist)
    elif not fromlist:
        # import a [as b]
        if '.' in name: # a.b
            base, rest = name.split('.', 1)
            # if a is already demand-loaded, add b to its submodule list
            if base in locals:
                if isinstance(locals[base], _demandmod):
                    locals[base]._extend(rest)
                return locals[base]
        return _demandmod(name, globals, locals)
    else:
        # from a import b,c,d
        mod = _origimport(name, globals, locals)
        # recurse down the module chain
        for comp in name.split('.')[1:]:
            mod = getattr(mod, comp)
        for x in fromlist:
            # set requested submodules for demand load
            if not(hasattr(mod, x)):
                setattr(mod, x, _demandmod(x, mod.__dict__, mod.__dict__))
        return mod

ignore = ['_hashlib', 'email.mime']

def enable():
    "enable global demand-loading of modules"
    __builtins__["__import__"] = _demandimport

def disable():
    "disable global demand-loading of modules"
    __builtins__["__import__"] = _origimport

