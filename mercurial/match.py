import util

class _match(object):
    def __init__(self, root, cwd, files, mf, ap):
        self._root = root
        self._cwd = cwd
        self._files = files
        self._fmap = dict.fromkeys(files)
        self.matchfn = mf
        self._anypats = ap
    def __call__(self, fn):
        return self.matchfn(fn)
    def __iter__(self):
        for f in self._files:
            yield f
    def bad(self, f, msg):
        return True
    def dir(self, f):
        pass
    def missing(self, f):
        pass
    def exact(self, f):
        return f in self._fmap
    def rel(self, f):
        return util.pathto(self._root, self._cwd, f)
    def files(self):
        return self._files
    def anypats(self):
        return self._anypats

class always(_match):
    def __init__(self, root, cwd):
        _match.__init__(self, root, cwd, [], lambda f: True, False)

class never(_match):
    def __init__(self, root, cwd):
        _match.__init__(self, root, cwd, [], lambda f: False, False)

class exact(_match):
    def __init__(self, root, cwd, files):
        _match.__init__(self, root, cwd, files, lambda f: f in files, False)

class match(_match):
    def __init__(self, root, cwd, patterns, include, exclude, default):
        f, mf, ap = util.matcher(root, cwd, patterns, include, exclude,
                                 None, default)
        _match.__init__(self, root, cwd, f, mf, ap)
