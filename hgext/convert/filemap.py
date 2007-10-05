# Copyright 2007 Bryan O'Sullivan <bos@serpentine.com>
#
# This software may be used and distributed according to the terms of
# the GNU General Public License, incorporated herein by reference.

import shlex
from mercurial.i18n import _
from mercurial import util

def rpairs(name):
    e = len(name)
    while e != -1:
        yield name[:e], name[e+1:]
        e = name.rfind('/', 0, e)

class filemapper(object):
    '''Map and filter filenames when importing.
    A name can be mapped to itself, a new name, or None (omit from new
    repository).'''

    def __init__(self, ui, path=None):
        self.ui = ui
        self.include = {}
        self.exclude = {}
        self.rename = {}
        if path:
            if self.parse(path):
                raise util.Abort(_('errors in filemap'))

    def parse(self, path):
        errs = 0
        def check(name, mapping, listname):
            if name in mapping:
                self.ui.warn(_('%s:%d: %r already in %s list\n') %
                             (lex.infile, lex.lineno, name, listname))
                return 1
            return 0
        lex = shlex.shlex(open(path), path, True)
        lex.wordchars += '!@#$%^&*()-=+[]{}|;:,./<>?'
        cmd = lex.get_token()
        while cmd:
            if cmd == 'include':
                name = lex.get_token()
                errs += check(name, self.exclude, 'exclude')
                self.include[name] = name
            elif cmd == 'exclude':
                name = lex.get_token()
                errs += check(name, self.include, 'include')
                errs += check(name, self.rename, 'rename')
                self.exclude[name] = name
            elif cmd == 'rename':
                src = lex.get_token()
                dest = lex.get_token()
                errs += check(src, self.exclude, 'exclude')
                self.rename[src] = dest
            elif cmd == 'source':
                errs += self.parse(lex.get_token())
            else:
                self.ui.warn(_('%s:%d: unknown directive %r\n') %
                             (lex.infile, lex.lineno, cmd))
                errs += 1
            cmd = lex.get_token()
        return errs

    def lookup(self, name, mapping):
        for pre, suf in rpairs(name):
            try:
                return mapping[pre], pre, suf
            except KeyError, err:
                pass
        return '', name, ''

    def __call__(self, name):
        if self.include:
            inc = self.lookup(name, self.include)[0]
        else:
            inc = name
        if self.exclude:
            exc = self.lookup(name, self.exclude)[0]
        else:
            exc = ''
        if not inc or exc:
            return None
        newpre, pre, suf = self.lookup(name, self.rename)
        if newpre:
            if newpre == '.':
                return suf
            if suf:
                return newpre + '/' + suf
            return newpre
        return name

    def active(self):
        return bool(self.include or self.exclude or self.rename)
