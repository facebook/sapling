"""Fixer that changes bytes % whatever to a function that actually formats
it."""

from lib2to3 import fixer_base
from lib2to3.fixer_util import is_tuple, Call, Comma, Name, touch_import

# XXX: Implementing a blacklist in 2to3 turned out to be more troublesome than
# blacklisting some modules inside the fixers. So, this is what I came with.

blacklist = ['mercurial/demandimport.py',
             'mercurial/py3kcompat.py',
             'mercurial/i18n.py',
            ]

def isnumberremainder(formatstr, data):
    try:
        if data.value.isdigit():
            return True
    except AttributeError:
        return False

class FixBytesmod(fixer_base.BaseFix):
    # XXX: There's one case (I suppose) I can't handle: when a remainder
    # operation like foo % bar is performed, I can't really know what the
    # contents of foo and bar are. I believe the best approach is to "correct"
    # the to-be-converted code and let bytesformatter handle that case in
    # runtime.
    PATTERN = '''
              term< formatstr=STRING '%' data=STRING > |
              term< formatstr=STRING '%' data=atom > |
              term< formatstr=NAME '%' data=any > |
              term< formatstr=any '%' data=any >
              '''

    def transform(self, node, results):
        if self.filename in blacklist:
            return
        elif self.filename == 'mercurial/util.py':
            touch_import('.', 'py3kcompat', node=node)

        formatstr = results['formatstr'].clone()
        data = results['data'].clone()
        formatstr.prefix = '' # remove spaces from start

        if isnumberremainder(formatstr, data):
            return

        # We have two possibilities:
        # 1- An identifier or name is passed, it is going to be a leaf, thus, we
        #    just need to copy its value as an argument to the formatter;
        # 2- A tuple is explicitly passed. In this case, we're gonna explode it
        # to pass to the formatter
        # TODO: Check for normal strings. They don't need to be translated

        if is_tuple(data):
            args = [formatstr, Comma().clone()] + \
                   [c.clone() for c in data.children[:]]
        else:
            args = [formatstr, Comma().clone(), data]

        call = Call(Name('bytesformatter', prefix=' '), args)
        return call

