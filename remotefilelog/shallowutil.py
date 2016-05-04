# shallowutil.py -- remotefilelog utilities
#
# Copyright 2014 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import errno, platform, os, subprocess, sys
from mercurial import filelog, util

def interposeclass(container, classname):
    '''Interpose a class into the hierarchies of all loaded subclasses. This
    function is intended for use as a decorator.

      import mymodule
      @replaceclass(mymodule, 'myclass')
      class mysubclass(mymodule.myclass):
          def foo(self):
              f = super(mysubclass, self).foo()
              return f + ' bar'

    Existing instances of the class being replaced will not have their
    __class__ modified, so call this function before creating any
    objects of the target type. Note that this doesn't actually replace the
    class in the module -- that can cause problems when using e.g. super()
    to call a method in the parent class. Instead, new instances should be
    created using a factory of some sort that this extension can override.
    '''
    def wrap(cls):
        oldcls = getattr(container, classname)
        oldbases = (oldcls,)
        newbases = (cls,)
        for subcls in oldcls.__subclasses__():
            if subcls is not cls:
                assert subcls.__bases__ == oldbases
                subcls.__bases__ = newbases
        return cls
    return wrap

def getcachekey(reponame, file, id):
    pathhash = util.sha1(file).hexdigest()
    return os.path.join(reponame, pathhash[:2], pathhash[2:], id)

def getlocalkey(file, id):
    pathhash = util.sha1(file).hexdigest()
    return os.path.join(pathhash, id)

def createrevlogtext(text, copyfrom=None, copyrev=None):
    """returns a string that matches the revlog contents in a
    traditional revlog
    """
    meta = {}
    if copyfrom or text.startswith('\1\n'):
        if copyfrom:
            meta['copy'] = copyfrom
            meta['copyrev'] = copyrev
        text = filelog.packmeta(meta, text)

    return text

def parsemeta(text):
    meta, size = filelog.parsemeta(text)
    if text.startswith('\1\n'):
        s = text.index('\1\n', 2)
        text = text[s + 2:]
    return meta or {}, text

def parsesize(raw):
    try:
        index = raw.index('\0')
        size = int(raw[:index])
    except ValueError:
        raise RuntimeError("corrupt cache data")
    return index, size

def ancestormap(raw):
    index, size = parsesize(raw)
    start = index + 1 + size

    mapping = {}
    while start < len(raw):
        divider = raw.index('\0', start + 80)

        currentnode = raw[start:(start + 20)]
        p1 = raw[(start + 20):(start + 40)]
        p2 = raw[(start + 40):(start + 60)]
        linknode = raw[(start + 60):(start + 80)]
        copyfrom = raw[(start + 80):divider]

        mapping[currentnode] = (p1, p2, linknode, copyfrom)
        start = divider + 1

    return mapping

def readfile(path):
    f = open(path, "r")
    try:
        result = f.read()

        # we should never have empty files
        if not result:
            os.remove(path)
            raise IOError("empty file: %s" % path)

        return result
    finally:
        f.close()

def writefile(path, content, readonly=False):
    dirname = os.path.dirname(path)
    if not os.path.exists(dirname):
        try:
            os.makedirs(dirname)
        except OSError as ex:
            if ex.errno != errno.EEXIST:
                raise

    # atomictempfile doesn't pick up the fact that we changed the umask, so we
    # need to set it manually.
    f = util.atomictempfile(path, 'w', createmode=~os.umask(0))
    try:
        f.write(content)
    finally:
        f.close()

    if readonly:
        os.chmod(path, 0o444)

def sortnodes(nodes, parentfunc):
    """Topologically sorts the nodes, using the parentfunc to find
    the parents of nodes."""
    nodes = set(nodes)
    childmap = {}
    parentmap = {}
    roots = []

    # Build a child and parent map
    for n in nodes:
        parents = [p for p in parentfunc(n) if p in nodes]
        parentmap[n] = set(parents)
        for p in parents:
            childmap.setdefault(p, set()).add(n)
        if not parents:
            roots.append(n)

    # Process roots, adding children to the queue as they become roots
    results = []
    while roots:
        n = roots.pop(0)
        results.append(n)
        if n in childmap:
            children = childmap[n]
            for c in children:
                childparents = parentmap[c]
                childparents.remove(n)
                if len(childparents) == 0:
                    # insert at the beginning, that way child nodes
                    # are likely to be output immediately after their
                    # parents.  This gives better compression results.
                    roots.insert(0, c)

    return results

# Copied from the hgext/logtoprocess.py extension
if platform.system() == 'Windows':
    # no fork on Windows, but we can create a detached process
    # https://msdn.microsoft.com/en-us/library/windows/desktop/ms684863.aspx
    # No stdlib constant exists for this value
    DETACHED_PROCESS = 0x00000008
    _creationflags = DETACHED_PROCESS | subprocess.CREATE_NEW_PROCESS_GROUP

    def runshellcommand(script, env):
        # we can't use close_fds *and* redirect stdin. I'm not sure that we
        # need to because the detached process has no console connection.
        subprocess.Popen(
            script, shell=True, env=env, close_fds=True,
            creationflags=_creationflags)
else:
    def runshellcommand(script, env):
        # double-fork to completely detach from the parent process
        # based on http://code.activestate.com/recipes/278731
        pid = os.fork()
        if pid:
            # parent
            return
        # subprocess.Popen() forks again, all we need to add is
        # flag the new process as a new session.
        if sys.version_info < (3, 2):
            newsession = {'preexec_fn': os.setsid}
        else:
            newsession = {'start_new_session': True}
        try:
            # connect stdin to devnull to make sure the subprocess can't
            # muck up that stream for mercurial.
            subprocess.Popen(
                script, shell=True, stdout=open(os.devnull, 'w'),
                stderr=open(os.devnull, 'w'), stdin=open(os.devnull, 'r'),
                env=env, close_fds=True, **newsession)
        finally:
            # mission accomplished, this child needs to exit and not
            # continue the hg process here.
            os._exit(0)
