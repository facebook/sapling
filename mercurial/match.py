import util

class match(object):
    def __init__(self, root, cwd, patterns, include, exclude, default):
        self._patterns = patterns
        self._root = root
        self._cwd = cwd
        self._include = include
        self._exclude = exclude
        f, mf, ap = util.matcher(root, cwd, patterns, include, exclude,
                                 self.src(), default)
        self._files = f
        self._fmap = dict.fromkeys(f)
        self._matchfn = mf
        self._anypats = ap
    def src(self):
        return None
    def __call__(self, fn):
        return self._matchfn(fn)
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

def always(root, cwd):
    return match(root, cwd, [], None, None, 'relpath')

def never(root, cwd):
    m = match(root, cwd, [], None, None, 'relpath')
    m._matchfn = lambda f: False
    return m

def exact(root, cwd, files):
    m = always(root, cwd)
    m._files = files
    m._fmap = dict.fromkeys(files)
    m._matchfn = m._fmap.has_key
    return m
