import inspect
import io
import types


def make_cffi(cls):
    """Decorator to add CFFI versions of each test method."""

    try:
        import zstd_cffi
    except ImportError:
        return cls

    # If CFFI version is available, dynamically construct test methods
    # that use it.

    for attr in dir(cls):
        fn = getattr(cls, attr)
        if not inspect.ismethod(fn) and not inspect.isfunction(fn):
            continue

        if not fn.__name__.startswith('test_'):
            continue

        name = '%s_cffi' % fn.__name__

        # Replace the "zstd" symbol with the CFFI module instance. Then copy
        # the function object and install it in a new attribute.
        if isinstance(fn, types.FunctionType):
            globs = dict(fn.__globals__)
            globs['zstd'] = zstd_cffi
            new_fn = types.FunctionType(fn.__code__, globs, name,
                                        fn.__defaults__, fn.__closure__)
            new_method = new_fn
        else:
            globs = dict(fn.__func__.func_globals)
            globs['zstd'] = zstd_cffi
            new_fn = types.FunctionType(fn.__func__.func_code, globs, name,
                                        fn.__func__.func_defaults,
                                        fn.__func__.func_closure)
            new_method = types.UnboundMethodType(new_fn, fn.im_self,
                                                 fn.im_class)

        setattr(cls, name, new_method)

    return cls


class OpCountingBytesIO(io.BytesIO):
    def __init__(self, *args, **kwargs):
        self._read_count = 0
        self._write_count = 0
        return super(OpCountingBytesIO, self).__init__(*args, **kwargs)

    def read(self, *args):
        self._read_count += 1
        return super(OpCountingBytesIO, self).read(*args)

    def write(self, data):
        self._write_count += 1
        return super(OpCountingBytesIO, self).write(data)
