# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# lz4revlog.py - lz4 delta compression for mercurial

"""store revlog deltas using lz4 compression

This extension uses the lz4 compression algorithm to store deltas,
rather than Mercurial's default of zlib compression.  lz4 offers much
faster decompression than zlib, at a cost of about 30% more disk
space.  The improvement in decompression speed leads to speedups in
many common operations, such as update and history traversal.

To use lz4 compression, a repository can be created from scratch or
converted from an existing repository, for example using :prog:`clone
--pull`.

The behaviour of Mercurial in an existing zlib-compressed repository
will not be affected by this extension.

To avoid use of lz4 when cloning or creating a new repository, use
:prog:`--config format.uselz4=no`.

Interop with other Mercurial repositories is generally not affected by
this extension.
"""

from __future__ import absolute_import

from bindings import lz4
from edenscm import extensions, localrepo, revlog, util


testedwith = "3.9.1"


def replaceclass(container, classname: str):
    """Replace a class with another in a module, and interpose it into
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
    """

    def wrap(cls):
        oldcls = getattr(container, classname)
        oldbases = (oldcls,)
        newbases = (cls,)
        for subcls in oldcls.__subclasses__():
            if subcls is not cls:
                assert subcls.__bases__ == oldbases
                subcls.__bases__ = newbases
        setattr(container, classname, cls)
        return cls

    return wrap


lz4compresshc = lz4.compresshc
lz4decompress = lz4.decompress


def requirements(orig, repo):
    reqs = orig(repo)
    if repo.ui.configbool("format", "uselz4", True):
        reqs.add("lz4revlog")
    return reqs


def uisetup(ui) -> None:
    if util.safehasattr(localrepo, "newreporequirements"):
        extensions.wrapfunction(localrepo, "newreporequirements", requirements)
    else:

        @replaceclass(localrepo, "localrepository")
        class lz4repo(localrepo.localrepository):
            def _baserequirements(self, create):
                reqs = super(lz4repo, self)._baserequirements(create)
                if create and self.ui.configbool("format", "uselz4", True):
                    reqs.append("lz4revlog")
                return reqs

    @replaceclass(revlog, "revlog")
    class lz4revlog(revlog.revlog):
        def __init__(self, opener, indexfile, **kwds):
            super(lz4revlog, self).__init__(opener, indexfile, **kwds)
            opts = getattr(opener, "options", None)
            self._lz4 = opts and "lz4revlog" in opts

        def compress(self, text):
            if util.safehasattr(self, "_lz4") and self._lz4:
                if not text:
                    return (b"", text)
                c = lz4compresshc(text)
                if len(text) <= len(c):
                    if bytes(text[0:1]) == b"\0":
                        return (b"", text)
                    return (b"u", text)
                return (b"", b"4" + c)
            return super(lz4revlog, self).compress(text)

        def decompress(self, bin):
            if not bin:
                return bin
            t = bytes(bin[0:1])
            if t == b"\0":
                return bin
            if t == b"4":
                return lz4decompress(bin[1:])
            return super(lz4revlog, self).decompress(bin)

    cls = localrepo.localrepository
    for reqs in "supportedformats openerreqs".split():
        getattr(cls, reqs).add("lz4revlog")
    if util.safehasattr(cls, "_basesupported"):
        # hg >= 2.8. Since we're good at falling back to the usual revlog, we
        # aren't going to bother with enabling ourselves per-repository.
        cls._basesupported.add("lz4revlog")
    else:
        # hg <= 2.7
        cls.supported.add("lz4revlog")
