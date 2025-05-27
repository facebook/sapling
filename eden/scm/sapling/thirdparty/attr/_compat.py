
import types


def isclass(klass):
    return isinstance(klass, type)


TYPE = "class"


def iteritems(d):
    return d.items()


def iterkeys(d):
    return d.keys()


def metadata_proxy(d):
    return types.MappingProxyType(dict(d))
