# ignore.py - ignored file handling for mercurial
#
# Copyright 2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from i18n import _
import util, re

_commentre = None

def _parselines(fp):
    for line in fp:
        if "#" in line:
            global _commentre
            if not _commentre:
                _commentre = re.compile(r'((^|[^\\])(\\\\)*)#.*')
            # remove comments prefixed by an even number of escapes
            line = _commentre.sub(r'\1', line)
            # fixup properly escaped comments that survived the above
            line = line.replace("\\#", "#")
        line = line.rstrip()
        if line:
            yield line

def ignore(root, files, warn):
    '''return the contents of .hgignore files as a list of patterns.

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

    syntaxes = {'re': 'relre:', 'regexp': 'relre:', 'glob': 'relglob:'}
    pats = {}
    for f in files:
        try:
            pats[f] = []
            fp = open(f)
            syntax = 'relre:'
            for line in _parselines(fp):
                if line.startswith('syntax:'):
                    s = line[7:].strip()
                    try:
                        syntax = syntaxes[s]
                    except KeyError:
                        warn(_("%s: ignoring invalid syntax '%s'\n") % (f, s))
                    continue
                pat = syntax + line
                for s, rels in syntaxes.items():
                    if line.startswith(rels):
                        pat = line
                        break
                    elif line.startswith(s+':'):
                        pat = rels + line[len(s)+1:]
                        break
                pats[f].append(pat)
        except IOError, inst:
            if f != files[0]:
                warn(_("skipping unreadable ignore file '%s': %s\n") %
                     (f, inst.strerror))

    allpats = []
    [allpats.extend(patlist) for patlist in pats.values()]
    if not allpats:
        return util.never

    try:
        files, ignorefunc, anypats = (
            util.matcher(root, inc=allpats, src='.hgignore'))
    except util.Abort:
        # Re-raise an exception where the src is the right file
        for f, patlist in pats.items():
            files, ignorefunc, anypats = (
                util.matcher(root, inc=patlist, src=f))

    return ignorefunc
