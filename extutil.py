# extutil.py - useful utility methods for extensions
#
# Copyright 2016 Facebook
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from mercurial.i18n import _

def replaceclass(container, classname):
    '''Replace a class with another in a module, and interpose it into
    the hierarchies of all loaded subclasses. This function is
    intended for use as a decorator.

      import mymodule
      @replaceclass(mymodule, 'myclass')
      class mysubclass(mymodule.myclass):
          def foo(self):
              f = super(mysubclass, self).foo()
              return f + ' bar'

    Existing instances of the class being replaced will not have their
    __class__ modified, so call this function before creating any
    objects of the target type.
    '''
    def wrap(cls):
        oldcls = getattr(container, classname)
        for subcls in oldcls.__subclasses__():
            if subcls is not cls:
                assert oldcls in subcls.__bases__
                newbases = [oldbase
                            for oldbase in subcls.__bases__
                            if oldbase != oldcls]
                newbases.append(cls)
                subcls.__bases__ = tuple(newbases)
        setattr(container, classname, cls)
        return cls
    return wrap

def getfilecache(cls, name):
    """Retrieve a filecache descriptor object from a class.

    Because these are descriptors that are executed even when accessed directly
    on the class, they can be accessed only through `cls.__dict__` , which in
    turn requires a full scan over cls.__mro__.

    This function can be dropped altogether once
    https://patchwork.mercurial-scm.org/patch/15541/ lands upstream.

    """
    for parent in cls.__mro__:
        fcdescr = cls.__dict__.get(name)
        if fcdescr is not None:
            return fcdescr

    raise AttributeError(
        _("type '%s' has no filecache descriptor '%s'") % (cls, name))
