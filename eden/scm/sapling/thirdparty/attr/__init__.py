

from . import converters, exceptions, filters, validators
from ._config import get_run_validators, set_run_validators
from ._funcs import asdict, assoc, astuple, evolve, has
from ._make import (
    NOTHING,
    Attribute,
    Factory,
    attr,
    attributes,
    fields,
    make_class,
    validate,
)


__version__ = "17.2.0"

__title__ = "attrs"
__description__ = "Classes Without Boilerplate"
__uri__ = "http://www.attrs.org/"
__doc__ = __description__ + " <" + __uri__ + ">"

__author__ = "Hynek Schlawack"
__email__ = "hs@ox.cx"

__license__ = "MIT"
__copyright__ = "Copyright (c) 2015 Hynek Schlawack"

s = attrs = attributes
ib = attrib = attr

__all__ = [
    "Attribute",
    "Factory",
    "NOTHING",
    "asdict",
    "assoc",
    "astuple",
    "attr",
    "attrib",
    "attributes",
    "attrs",
    "converters",
    "evolve",
    "exceptions",
    "fields",
    "filters",
    "get_run_validators",
    "has",
    "ib",
    "make_class",
    "s",
    "set_run_validators",
    "validate",
    "validators",
]
