from __future__ import absolute_import, print_function

from mercurial import extensions

def genwrapper(x):
    def f(orig, *args, **kwds):
        return [x] + orig(*args, **kwds)
    f.x = x
    return f

def getid(wrapper):
    return getattr(wrapper, 'x', '-')

wrappers = [genwrapper(i) for i in range(5)]

class dummyclass(object):
    def getstack(self):
        return ['orig']

dummy = dummyclass()

def batchwrap(wrappers):
    for w in wrappers:
        extensions.wrapfunction(dummy, 'getstack', w)
        print('wrap %d: %s' % (getid(w), dummy.getstack()))

def batchunwrap(wrappers):
    for w in wrappers:
        result = None
        try:
            result = extensions.unwrapfunction(dummy, 'getstack', w)
            msg = str(dummy.getstack())
        except (ValueError, IndexError) as e:
            msg = e.__class__.__name__
        print('unwrap %s: %s: %s' % (getid(w), getid(result), msg))

batchwrap(wrappers + [wrappers[0]])
batchunwrap([(wrappers[i] if i >= 0 else None)
             for i in [3, None, 0, 4, 0, 2, 1, None]])
