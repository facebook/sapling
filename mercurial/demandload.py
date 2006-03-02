'''Demand load modules when used, not when imported.'''

__author__ = '''Copyright 2006 Vadim Gelfer <vadim.gelfer@gmail.com>.
This software may be used and distributed according to the terms
of the GNU General Public License, incorporated herein by reference.'''

# this is based on matt's original demandload module.  it is a
# complete rewrite.  some time, we may need to support syntax of
# "import foo as bar".

class _importer(object):
    '''import a module.  it is not imported until needed, and is
    imported at most once per scope.'''

    def __init__(self, scope, modname, fromlist):
        '''scope is context (globals() or locals()) in which import
        should be made.  modname is name of module to import.
        fromlist is list of modules for "from foo import ..."
        emulation.'''

        self.scope = scope
        self.modname = modname
        self.fromlist = fromlist
        self.mod = None

    def module(self):
        '''import the module if needed, and return.'''
        if self.mod is None:
            self.mod = __import__(self.modname, self.scope, self.scope,
                                  self.fromlist)
            del self.modname, self.fromlist
        return self.mod

class _replacer(object):
    '''placeholder for a demand loaded module. demandload puts this in
    a target scope.  when an attribute of this object is looked up,
    this object is replaced in the target scope with the actual
    module.

    we use __getattribute__ to avoid namespace clashes between
    placeholder object and real module.'''

    def __init__(self, importer, target):
        self.importer = importer
        self.target = target
        # consider case where we do this:
        #   demandload(globals(), 'foo.bar foo.quux')
        # foo will already exist in target scope when we get to
        # foo.quux.  so we remember that we will need to demandload
        # quux into foo's scope when we really load it.
        self.later = []

    def module(self):
        return object.__getattribute__(self, 'importer').module()

    def __getattribute__(self, key):
        '''look up an attribute in a module and return it. replace the
        name of the module in the caller\'s dict with the actual
        module.'''

        module = object.__getattribute__(self, 'module')()
        target = object.__getattribute__(self, 'target')
        importer = object.__getattribute__(self, 'importer')
        later = object.__getattribute__(self, 'later')

        if later:
            demandload(module.__dict__, ' '.join(later))

        importer.scope[target] = module

        return getattr(module, key)

class _replacer_from(_replacer):
    '''placeholder for a demand loaded module.  used for "from foo
    import ..." emulation. semantics of this are different than
    regular import, so different implementation needed.'''

    def module(self):
        importer = object.__getattribute__(self, 'importer')
        target = object.__getattribute__(self, 'target')

        return getattr(importer.module(), target)

def demandload(scope, modules):
    '''import modules into scope when each is first used.

    scope should be the value of globals() in the module calling this
    function, or locals() in the calling function.

    modules is a string listing module names, separated by white
    space.  names are handled like this:

    foo            import foo
    foo bar        import foo, bar
    foo.bar        import foo.bar
    foo:bar        from foo import bar
    foo:bar,quux   from foo import bar, quux
    foo.bar:quux   from foo.bar import quux'''

    for mod in modules.split():
        col = mod.find(':')
        if col >= 0:
            fromlist = mod[col+1:].split(',')
            mod = mod[:col]
        else:
            fromlist = []
        importer = _importer(scope, mod, fromlist)
        if fromlist:
            for name in fromlist:
                scope[name] = _replacer_from(importer, name)
        else:
            dot = mod.find('.')
            if dot >= 0:
                basemod = mod[:dot]
                val = scope.get(basemod)
                # if base module has already been demandload()ed,
                # remember to load this submodule into its namespace
                # when needed.
                if isinstance(val, _replacer):
                    later = object.__getattribute__(val, 'later')
                    later.append(mod[dot+1:])
                    continue
            else:
                basemod = mod
            scope[basemod] = _replacer(importer, basemod)
