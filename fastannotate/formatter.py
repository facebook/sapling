# Copyright 2016-present Facebook. All Rights Reserved.
#
# format: defines the format used to output annotate result
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from mercurial import (
    encoding,
    formatter,
    node,
    util,
)

# imitating mercurial.commands.annotate, not using the vanilla formatter since
# the data structures are a bit different, and we have some fast paths.
class defaultformatter(object):
    """the default formatter that does leftpad and support some common flags"""

    def __init__(self, ui, repo, opts):
        if ui.quiet:
            datefunc = util.shortdate
        else:
            datefunc = util.datestr
        datefunc = util.cachefunc(datefunc)
        getctx = util.cachefunc(lambda x: repo[x[0]])

        # opt name, separator, raw value (for json/plain), encoder (for plain)
        opmap = [('user', ' ', lambda x: getctx(x).user(), ui.shortuser),
                 ('number', ' ', lambda x: getctx(x).rev(), str),
                 ('changeset', ' ', lambda x: self._hexfunc(x[0]), str),
                 ('date', ' ', lambda x: getctx(x).date(), datefunc),
                 ('file', ' ', lambda x: x[2], str),
                 ('line_number', ':', lambda x: x[1] + 1, str)]
        fieldnamemap = {'number': 'rev', 'changeset': 'node'}
        funcmap = [(get, sep, fieldnamemap.get(op, op), enc)
                   for op, sep, get, enc in opmap
                   if opts.get(op)]
        # no separator for first column
        funcmap[0] = list(funcmap[0])
        funcmap[0][1] = ''

        self.ui = ui
        self.opts = opts
        self.funcmap = funcmap

    def write(self, annotatedresult, lines=None, existinglines=None):
        """(annotateresult, [str], set([rev, linenum])) -> None. write output.
        annotateresult can be [(node, linenum, path)], or [(node, linenum)]
        """
        pieces = [] # [[str]]
        maxwidths = [] # [int]

        # calculate padding
        for f, sep, name, enc in self.funcmap:
            l = [enc(f(x)) for x in annotatedresult]
            pieces.append(l)
            widths = map(encoding.colwidth, set(l))
            maxwidth = (max(widths) if widths else 0)
            maxwidths.append(maxwidth)

        # buffered output
        result = ''
        for i in xrange(len(annotatedresult)):
            for j, p in enumerate(pieces):
                sep = self.funcmap[j][1]
                padding = ' ' * (maxwidths[j] - len(p[i]))
                result += sep + padding + p[i]
            if lines:
                if existinglines is None:
                    result += ': ' + lines[i]
                else: # extra formatting showing whether a line exists
                    key = (annotatedresult[i][0], annotatedresult[i][1])
                    if key in existinglines:
                        result += ':  ' + lines[i]
                    else:
                        result += ': ' + self.ui.label('-' + lines[i],
                                                       'diff.deleted')

            if result[-1] != '\n':
                result += '\n'

        self.ui.write(result)

    @util.propertycache
    def _hexfunc(self):
        if self.ui.debugflag or self.opts.get('long_hash'):
            return node.hex
        else:
            return node.short

    def end(self):
        pass

class jsonformatter(defaultformatter):
    def __init__(self, ui, repo, opts):
        super(jsonformatter, self).__init__(ui, repo, opts)
        self.ui.write('[')
        self.needcomma = False

    def write(self, annotatedresult, lines=None, existinglines=None):
        if annotatedresult:
            self._writecomma()

        pieces = [(name, map(f, annotatedresult))
                  for f, sep, name, enc in self.funcmap]
        if lines is not None:
            pieces.append(('line', lines))
        pieces.sort()

        seps = [','] * len(pieces[:-1]) + ['']

        result = ''
        lasti = len(annotatedresult) - 1
        for i in xrange(len(annotatedresult)):
            result += '\n {\n'
            for j, p in enumerate(pieces):
                k, vs = p
                result += ('  "%s": %s%s\n'
                           % (k, formatter._jsonifyobj(vs[i]), seps[j]))
            result += ' }%s' % ('' if i == lasti else ',')
        if lasti >= 0:
            self.needcomma = True

        self.ui.write(result)

    def _writecomma(self):
        if self.needcomma:
            self.ui.write(',')
            self.needcomma = False

    @util.propertycache
    def _hexfunc(self):
        return node.hex

    def end(self):
        self.ui.write('\n]\n')
