# ignore.py - ignored file handling for mercurial
#
# Copyright 2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from i18n import _
import util, match

def readpats(root, files, warn):
    '''return a dict mapping ignore-file-name to list-of-patterns'''

    pats = {}
    for f in files:
        if f in pats:
            continue
        try:
            pats[f] = match.readpatternfile(f, warn)
        except IOError, inst:
            warn(_("skipping unreadable ignore file '%s': %s\n") %
                 (f, inst.strerror))

    return [(f, pats[f]) for f in files if f in pats]

def ignore(root, files, warn):
    '''return matcher covering patterns in 'files'.

    the files parsed for patterns include:
    .hgignore in the repository root
    any additional files specified in the [ui] section of ~/.hgrc

    trailing white space is dropped.
    the escape character is backslash.
    comments start with #.
    empty lines are skipped.

    lines can be of the following formats:

    syntax: regexp # defaults following lines to non-rooted regexps
    syntax: glob   # defaults following lines to non-rooted globs
    re:pattern     # non-rooted regular expression
    glob:pattern   # non-rooted glob
    pattern        # pattern of the current default type'''

    pats = readpats(root, files, warn)

    allpats = []
    for f, patlist in pats:
        allpats.extend(patlist)
    if not allpats:
        return util.never

    try:
        ignorefunc = match.match(root, '', [], allpats)
    except util.Abort:
        # Re-raise an exception where the src is the right file
        for f, patlist in pats:
            try:
                match.match(root, '', [], patlist)
            except util.Abort, inst:
                raise util.Abort('%s: %s' % (f, inst[0]))

    return ignorefunc
