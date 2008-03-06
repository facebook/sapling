# win32text.py - LF <-> CRLF translation utilities for Windows users
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.
#
# To perform automatic newline conversion, use:
#
# [extensions]
# hgext.win32text =
# [encode]
# ** = cleverencode:
# [decode]
# ** = cleverdecode:
#
# If not doing conversion, to make sure you do not commit CRLF by accident:
#
# [hooks]
# pretxncommit.crlf = python:hgext.win32text.forbidcrlf
#
# To do the same check on a server to prevent CRLF from being pushed or pulled:
#
# [hooks]
# pretxnchangegroup.crlf = python:hgext.win32text.forbidcrlf

from mercurial.i18n import gettext as _
from mercurial.node import bin, short
import re

# regexp for single LF without CR preceding.
re_single_lf = re.compile('(^|[^\r])\n', re.MULTILINE)

def dumbdecode(s, cmd, ui=None, repo=None, filename=None, **kwargs):
    # warn if already has CRLF in repository.
    # it might cause unexpected eol conversion.
    # see issue 302:
    #   http://www.selenic.com/mercurial/bts/issue302
    if '\r\n' in s and ui and filename and repo:
        ui.warn(_('WARNING: %s already has CRLF line endings\n'
                  'and does not need EOL conversion by the win32text plugin.\n'
                  'Before your next commit, please reconsider your '
                  'encode/decode settings in \nMercurial.ini or %s.\n') %
                (filename, repo.join('hgrc')))
    # replace single LF to CRLF
    return re_single_lf.sub('\\1\r\n', s)

def dumbencode(s, cmd):
    return s.replace('\r\n', '\n')

def clevertest(s, cmd):
    if '\0' in s: return False
    return True

def cleverdecode(s, cmd, **kwargs):
    if clevertest(s, cmd):
        return dumbdecode(s, cmd, **kwargs)
    return s

def cleverencode(s, cmd):
    if clevertest(s, cmd):
        return dumbencode(s, cmd)
    return s

_filters = {
    'dumbdecode:': dumbdecode,
    'dumbencode:': dumbencode,
    'cleverdecode:': cleverdecode,
    'cleverencode:': cleverencode,
    }

def forbidcrlf(ui, repo, hooktype, node, **kwargs):
    halt = False
    for rev in xrange(repo.changelog.rev(bin(node)), repo.changelog.count()):
        c = repo.changectx(rev)
        for f in c.files():
            if f not in c:
                continue
            data = c[f].data()
            if '\0' not in data and '\r\n' in data:
                if not halt:
                    ui.warn(_('Attempt to commit or push text file(s) '
                              'using CRLF line endings\n'))
                ui.warn(_('in %s: %s\n') % (short(c.node()), f))
                halt = True
    if halt and hooktype == 'pretxnchangegroup':
        ui.warn(_('\nTo prevent this mistake in your local repository,\n'
                  'add to Mercurial.ini or .hg/hgrc:\n'
                  '\n'
                  '[hooks]\n'
                  'pretxncommit.crlf = python:hgext.win32text.forbidcrlf\n'
                  '\n'
                  'and also consider adding:\n'
                  '\n'
                  '[extensions]\n'
                  'hgext.win32text =\n'
                  '[encode]\n'
                  '** = cleverencode:\n'
                  '[decode]\n'
                  '** = cleverdecode:\n'))
    return halt

def reposetup(ui, repo):
    if not repo.local():
        return
    for name, fn in _filters.iteritems():
        repo.adddatafilter(name, fn)

