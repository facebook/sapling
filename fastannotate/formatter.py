# Copyright 2016-present Facebook. All Rights Reserved.
#
# format: defines the format used to output annotate result
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from mercurial import (
    encoding,
    node,
    util,
)

# extracted from mercurial.commands.annotate
class defaultformatter(object):
    """the default formatter that does leftpad and support some common flags"""

    def __init__(self, ui, repo, opts):
        if ui.quiet:
            datefunc = util.shortdate
        else:
            datefunc = util.datestr
        if ui.debugflag or opts.get('long_hash'):
            hexfunc = node.hex
        else:
            hexfunc = node.short
        datefunc = util.cachefunc(datefunc)
        getctx = util.cachefunc(lambda x: repo[x[0]])

        opmap = [('user', ' ', lambda x: ui.shortuser(getctx(x).user())),
                 ('number', ' ', lambda x: str(getctx(x).rev())),
                 ('changeset', ' ', lambda x: hexfunc(x[0])),
                 ('date', ' ', lambda x: datefunc(getctx(x).date())),
                 ('file', ' ', lambda x: x[2]),
                 ('line_number', ':', lambda x: str(x[1] + 1))]

        funcmap = [(get, sep) for op, sep, get in opmap
                   if opts.get(op)]
        funcmap[0] = (funcmap[0][0], '') # no separator for first column

        self.ui = ui
        self.funcmap = funcmap

    def write(self, annotatedresult, lines=None, existinglines=None):
        """(annotateresult, [str], set([rev, linenum])) -> None. write output.
        annotateresult can be [(node, linenum, path)], or [(node, linenum)]
        """
        pieces = [] # [[str]]
        maxwidths = [] # [int]

        # calculate
        for f, sep in self.funcmap:
            l = map(f, annotatedresult)
            pieces.append(l)
            widths = map(encoding.colwidth, l)
            maxwidth = (max(widths) if widths else 0)
            maxwidths.append(maxwidth)

        # output
        for i in xrange(len(annotatedresult)):
            msg = ''
            for j, p in enumerate(pieces):
                sep = self.funcmap[j][1]
                padding = ' ' * (maxwidths[j] - len(p[i]))
                msg += sep + padding + p[i]
            if lines:
                if existinglines is None:
                    msg += ': ' + lines[i]
                else: # extra formatting showing whether a line exists
                    key = (annotatedresult[i][0], annotatedresult[i][1])
                    if key in existinglines:
                        msg += ':  ' + lines[i]
                    else:
                        msg += ': ' + self.ui.label('-' + lines[i],
                                                    'diff.deleted')

            if msg[-1] != '\n':
                msg += '\n'
            self.ui.write(msg)
