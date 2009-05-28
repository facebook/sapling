''' Module for self-contained maps. '''

import os
from mercurial import util as hgutil

class AuthorMap(dict):
    '''A mapping from Subversion-style authors to Mercurial-style
    authors, and back. The data is stored persistently on disk.

    If the 'hgsubversion.defaultauthors' configuration option is set to false,
    attempting to obtain an unknown author will fail with an Abort.
    '''

    def __init__(self, ui, path, defaulthost=None):
        '''Initialise a new AuthorMap.

        The ui argument is used to print diagnostic messages.

        The path argument is the location of the backing store,
        typically .hg/authormap.
        '''
        self.ui = ui
        self.path = path
        if defaulthost:
            self.defaulthost = '@%s' % defaulthost.lstrip('@')
        else:
            self.defaulthost = ''
        self.super = super(AuthorMap, self)
        self.super.__init__()
        self.load(path)

    def load(self, path):
        ''' Load mappings from a file at the specified path. '''
        if not os.path.exists(path):
            return
        self.ui.note('reading authormap from %s\n' % path)
        f = open(path, 'r')
        for number, line in enumerate(f):

            if not line.strip():
                continue

            try:
                src, dst = line.split('=', 1)
            except (IndexError, ValueError):
                msg = 'ignoring line %i in author map %s: %s\n'
                self.ui.warn(msg % (number, path, line.rstrip()))
                continue

            src = src.strip()
            dst = dst.strip()
            if src in self and dst != self[src]:
                msg = 'overriding author: "%s" to "%s" (%s)\n'
                self.ui.warn(msg % (self[src], dst, src))
            else:
                self[src] = dst

        f.close()

    def __setitem__(self, key, value):
        ''' Similar to dict.__setitem__, but also updates the new mapping in the
        backing store. '''
        self.super.__setitem__(key, value)
        self.ui.debug('adding author %s to author map\n' % self.path)
        f = open(self.path, 'w+')
        for k, v in self.iteritems():
            f.write("%s=%s\n" % (k, v))
        f.close()

    def __getitem__(self, author):
        ''' Similar to dict.__getitem__, except in case of an unknown author.
        In such cases, a new value is generated and added to the dictionary
        as well as the backing store. '''
        if author in self:
            result = self.super.__getitem__(author)
        elif self.ui.configbool('hgsubversion', 'defaultauthors', True):
            self[author] = result = '%s%s' % (author, self.defaulthost)
            msg = 'substituting author "%s" for default "%s"\n'
            self.ui.note(msg % (author, result))
        else:
            msg = 'author %s has no entry in the author map!'
            raise hgutil.Abort(msg % author)
        self.ui.debug('mapping author "%s" to "%s"\n' % (author, result))
        return result

    def reverselookup(self, author):
        for svnauthor, hgauthor in self.iteritems():
            if author == hgauthor:
                return svnauthor
        else:
            # Mercurial incorrectly splits at e.g. '.', so we roll our own.
            return author.rsplit('@', 1)[0]
