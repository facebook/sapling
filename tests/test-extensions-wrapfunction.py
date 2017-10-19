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

wrap0 = extensions.wrappedfunction(dummy, 'getstack', wrappers[0])
wrap1 = extensions.wrappedfunction(dummy, 'getstack', wrappers[1])

# Use them in a different order from how they were created to check that
# the wrapping happens in __enter__, not in __init__
print('context manager', dummy.getstack())
with wrap1:
    print('context manager', dummy.getstack())
    with wrap0:
        print('context manager', dummy.getstack())
        # Bad programmer forgets to unwrap the function, but the context
        # managers still unwrap their wrappings.
        extensions.wrapfunction(dummy, 'getstack', wrappers[2])
        print('context manager', dummy.getstack())
    print('context manager', dummy.getstack())
print('context manager', dummy.getstack())

# Wrap callable object which has no __name__
class callableobj(object):
    def __call__(self):
        return ['orig']
dummy.cobj = callableobj()
extensions.wrapfunction(dummy, 'cobj', wrappers[0])
print('wrap callable object', dummy.cobj())
