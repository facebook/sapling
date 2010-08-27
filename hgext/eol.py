"""automatically manage newlines in repository files

This extension allows you to manage the type of line endings (CRLF or
LF) that are used in the repository and in the local working
directory. That way you can get CRLF line endings on Windows and LF on
Unix/Mac, thereby letting everybody use their OS native line endings.

The extension reads its configuration from a versioned ``.hgeol``
configuration file every time you run an ``hg`` command. The
``.hgeol`` file use the same syntax as all other Mercurial
configuration files. It uses two sections, ``[patterns]`` and
``[repository]``.

The ``[patterns]`` section specifies the line endings used in the
working directory. The format is specified by a file pattern. The
first match is used, so put more specific patterns first. The
available line endings are ``LF``, ``CRLF``, and ``BIN``.

Files with the declared format of ``CRLF`` or ``LF`` are always
checked out in that format and files declared to be binary (``BIN``)
are left unchanged. Additionally, ``native`` is an alias for the
platform's default line ending: ``LF`` on Unix (including Mac OS X)
and ``CRLF`` on Windows. Note that ``BIN`` (do nothing to line
endings) is Mercurial's default behaviour; it is only needed if you
need to override a later, more general pattern.

The optional ``[repository]`` section specifies the line endings to
use for files stored in the repository. It has a single setting,
``native``, which determines the storage line endings for files
declared as ``native`` in the ``[patterns]`` section. It can be set to
``LF`` or ``CRLF``. The default is ``LF``. For example, this means
that on Windows, files configured as ``native`` (``CRLF`` by default)
will be converted to ``LF`` when stored in the repository. Files
declared as ``LF``, ``CRLF``, or ``BIN`` in the ``[patterns]`` section
are always stored as-is in the repository.

Example versioned ``.hgeol`` file::

  [patterns]
  **.py = native
  **.vcproj = CRLF
  **.txt = native
  Makefile = LF
  **.jpg = BIN

  [repository]
  native = LF

The extension uses an optional ``[eol]`` section in your hgrc file
(not the ``.hgeol`` file) for settings that control the overall
behavior. There are two settings:

- ``eol.native`` (default ``os.linesep``) can be set to ``LF`` or
  ``CRLF`` override the default interpretation of ``native`` for
  checkout. This can be used with :hg:`archive` on Unix, say, to
  generate an archive where files have line endings for Windows.

- ``eol.only-consistent`` (default True) can be set to False to make
  the extension convert files with inconsistent EOLs. Inconsistent
  means that there is both ``CRLF`` and ``LF`` present in the file.
  Such files are normally not touched under the assumption that they
  have mixed EOLs on purpose.

See :hg:`help patterns` for more information about the glob patterns
used.
"""

from mercurial.i18n import _
from mercurial import util, config, extensions, match
import re, os

# Matches a lone LF, i.e., one that is not part of CRLF.
singlelf = re.compile('(^|[^\r])\n')
# Matches a single EOL which can either be a CRLF where repeated CR
# are removed or a LF. We do not care about old Machintosh files, so a
# stray CR is an error.
eolre = re.compile('\r*\n')


def inconsistenteol(data):
    return '\r\n' in data and singlelf.search(data)

def tolf(s, params, ui, **kwargs):
    """Filter to convert to LF EOLs."""
    if util.binary(s):
        return s
    if ui.configbool('eol', 'only-consistent', True) and inconsistenteol(s):
        return s
    return eolre.sub('\n', s)

def tocrlf(s, params, ui, **kwargs):
    """Filter to convert to CRLF EOLs."""
    if util.binary(s):
        return s
    if ui.configbool('eol', 'only-consistent', True) and inconsistenteol(s):
        return s
    return eolre.sub('\r\n', s)

def isbinary(s, params):
    """Filter to do nothing with the file."""
    return s

filters = {
    'to-lf': tolf,
    'to-crlf': tocrlf,
    'is-binary': isbinary,
}


def hook(ui, repo, node, hooktype, **kwargs):
    """verify that files have expected EOLs"""
    files = set()
    for rev in xrange(repo[node].rev(), len(repo)):
        files.update(repo[rev].files())
    tip = repo['tip']
    for f in files:
        if f not in tip:
            continue
        for pattern, target in ui.configitems('encode'):
            if match.match(repo.root, '', [pattern])(f):
                data = tip[f].data()
                if target == "to-lf" and "\r\n" in data:
                    raise util.Abort(_("%s should not have CRLF line endings")
                                     % f)
                elif target == "to-crlf" and singlelf.search(data):
                    raise util.Abort(_("%s should not have LF line endings")
                                     % f)


def preupdate(ui, repo, hooktype, parent1, parent2):
    #print "preupdate for %s: %s -> %s" % (repo.root, parent1, parent2)
    repo.readhgeol(parent1)
    return False

def uisetup(ui):
    ui.setconfig('hooks', 'preupdate.eol', preupdate)

def extsetup(ui):
    try:
        extensions.find('win32text')
        raise util.Abort(_("the eol extension is incompatible with the "
                           "win32text extension"))
    except KeyError:
        pass


def reposetup(ui, repo):
    #print "reposetup for", repo.root

    if not repo.local():
        return
    for name, fn in filters.iteritems():
        repo.adddatafilter(name, fn)

    ui.setconfig('patch', 'eol', 'auto')

    class eolrepo(repo.__class__):

        _decode = {'LF': 'to-lf', 'CRLF': 'to-crlf', 'BIN': 'is-binary'}
        _encode = {'LF': 'to-lf', 'CRLF': 'to-crlf', 'BIN': 'is-binary'}

        def readhgeol(self, node=None, data=None):
            if data is None:
                try:
                    if node is None:
                        data = self.wfile('.hgeol').read()
                    else:
                        data = self[node]['.hgeol'].data()
                except (IOError, LookupError):
                    return None

            if self.ui.config('eol', 'native', os.linesep) in ('LF', '\n'):
                self._decode['NATIVE'] = 'to-lf'
            else:
                self._decode['NATIVE'] = 'to-crlf'

            eol = config.config()
            eol.parse('.hgeol', data)

            if eol.get('repository', 'native') == 'CRLF':
                self._encode['NATIVE'] = 'to-crlf'
            else:
                self._encode['NATIVE'] = 'to-lf'

            for pattern, style in eol.items('patterns'):
                key = style.upper()
                try:
                    self.ui.setconfig('decode', pattern, self._decode[key])
                    self.ui.setconfig('encode', pattern, self._encode[key])
                except KeyError:
                    self.ui.warn(_("ignoring unknown EOL style '%s' from %s\n")
                                 % (style, eol.source('patterns', pattern)))

            include = []
            exclude = []
            for pattern, style in eol.items('patterns'):
                key = style.upper()
                if key == 'BIN':
                    exclude.append(pattern)
                else:
                    include.append(pattern)

            # This will match the files for which we need to care
            # about inconsistent newlines.
            return match.match(self.root, '', [], include, exclude)

        def _hgcleardirstate(self):
            self._eolfile = self.readhgeol() or self.readhgeol('tip')

            if not self._eolfile:
                self._eolfile = util.never
                return

            try:
                cachemtime = os.path.getmtime(self.join("eol.cache"))
            except OSError:
                cachemtime = 0

            try:
                eolmtime = os.path.getmtime(self.wjoin(".hgeol"))
            except OSError:
                eolmtime = 0

            if eolmtime > cachemtime:
                ui.debug("eol: detected change in .hgeol\n")
                # TODO: we could introduce a method for this in dirstate.
                wlock = None
                try:
                    wlock = self.wlock()
                    for f, e in self.dirstate._map.iteritems():
                        self.dirstate._map[f] = (e[0], e[1], -1, 0)
                    self.dirstate._dirty = True
                    # Touch the cache to update mtime. TODO: are we sure this
                    # always enought to update the mtime, or should we write a
                    # bit to the file?
                    self.opener("eol.cache", "w").close()
                finally:
                    if wlock is not None:
                        wlock.release()

        def commitctx(self, ctx, error=False):
            for f in sorted(ctx.added() + ctx.modified()):
                if not self._eolfile(f):
                    continue
                data = ctx[f].data()
                if util.binary(data):
                    # We should not abort here, since the user should
                    # be able to say "** = native" to automatically
                    # have all non-binary files taken care of.
                    continue
                if inconsistenteol(data):
                    raise util.Abort(_("inconsistent newline style "
                                       "in %s\n" % f))
            return super(eolrepo, self).commitctx(ctx, error)
    repo.__class__ = eolrepo
    repo._hgcleardirstate()
