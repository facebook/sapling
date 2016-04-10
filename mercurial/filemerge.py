# filemerge.py - file-level merge handling for Mercurial
#
# Copyright 2006, 2007, 2008 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import filecmp
import os
import re
import tempfile

from .i18n import _
from .node import nullid, short

from . import (
    error,
    formatter,
    match,
    scmutil,
    simplemerge,
    tagmerge,
    templatekw,
    templater,
    util,
)

def _toolstr(ui, tool, part, default=""):
    return ui.config("merge-tools", tool + "." + part, default)

def _toolbool(ui, tool, part, default=False):
    return ui.configbool("merge-tools", tool + "." + part, default)

def _toollist(ui, tool, part, default=[]):
    return ui.configlist("merge-tools", tool + "." + part, default)

internals = {}
# Merge tools to document.
internalsdoc = {}

# internal tool merge types
nomerge = None
mergeonly = 'mergeonly'  # just the full merge, no premerge
fullmerge = 'fullmerge'  # both premerge and merge

class absentfilectx(object):
    """Represents a file that's ostensibly in a context but is actually not
    present in it.

    This is here because it's very specific to the filemerge code for now --
    other code is likely going to break with the values this returns."""
    def __init__(self, ctx, f):
        self._ctx = ctx
        self._f = f

    def path(self):
        return self._f

    def size(self):
        return None

    def data(self):
        return None

    def filenode(self):
        return nullid

    _customcmp = True
    def cmp(self, fctx):
        """compare with other file context

        returns True if different from fctx.
        """
        return not (fctx.isabsent() and
                    fctx.ctx() == self.ctx() and
                    fctx.path() == self.path())

    def flags(self):
        return ''

    def changectx(self):
        return self._ctx

    def isbinary(self):
        return False

    def isabsent(self):
        return True

def internaltool(name, mergetype, onfailure=None, precheck=None):
    '''return a decorator for populating internal merge tool table'''
    def decorator(func):
        fullname = ':' + name
        func.__doc__ = "``%s``\n" % fullname + func.__doc__.strip()
        internals[fullname] = func
        internals['internal:' + name] = func
        internalsdoc[fullname] = func
        func.mergetype = mergetype
        func.onfailure = onfailure
        func.precheck = precheck
        return func
    return decorator

def _findtool(ui, tool):
    if tool in internals:
        return tool
    return findexternaltool(ui, tool)

def findexternaltool(ui, tool):
    for kn in ("regkey", "regkeyalt"):
        k = _toolstr(ui, tool, kn)
        if not k:
            continue
        p = util.lookupreg(k, _toolstr(ui, tool, "regname"))
        if p:
            p = util.findexe(p + _toolstr(ui, tool, "regappend"))
            if p:
                return p
    exe = _toolstr(ui, tool, "executable", tool)
    return util.findexe(util.expandpath(exe))

def _picktool(repo, ui, path, binary, symlink, changedelete):
    def supportscd(tool):
        return tool in internals and internals[tool].mergetype == nomerge

    def check(tool, pat, symlink, binary, changedelete):
        tmsg = tool
        if pat:
            tmsg += " specified for " + pat
        if not _findtool(ui, tool):
            if pat: # explicitly requested tool deserves a warning
                ui.warn(_("couldn't find merge tool %s\n") % tmsg)
            else: # configured but non-existing tools are more silent
                ui.note(_("couldn't find merge tool %s\n") % tmsg)
        elif symlink and not _toolbool(ui, tool, "symlink"):
            ui.warn(_("tool %s can't handle symlinks\n") % tmsg)
        elif binary and not _toolbool(ui, tool, "binary"):
            ui.warn(_("tool %s can't handle binary\n") % tmsg)
        elif changedelete and not supportscd(tool):
            # the nomerge tools are the only tools that support change/delete
            # conflicts
            pass
        elif not util.gui() and _toolbool(ui, tool, "gui"):
            ui.warn(_("tool %s requires a GUI\n") % tmsg)
        else:
            return True
        return False

    # internal config: ui.forcemerge
    # forcemerge comes from command line arguments, highest priority
    force = ui.config('ui', 'forcemerge')
    if force:
        toolpath = _findtool(ui, force)
        if changedelete and not supportscd(toolpath):
            return ":prompt", None
        else:
            if toolpath:
                return (force, util.shellquote(toolpath))
            else:
                # mimic HGMERGE if given tool not found
                return (force, force)

    # HGMERGE takes next precedence
    hgmerge = os.environ.get("HGMERGE")
    if hgmerge:
        if changedelete and not supportscd(hgmerge):
            return ":prompt", None
        else:
            return (hgmerge, hgmerge)

    # then patterns
    for pat, tool in ui.configitems("merge-patterns"):
        mf = match.match(repo.root, '', [pat])
        if mf(path) and check(tool, pat, symlink, False, changedelete):
            toolpath = _findtool(ui, tool)
            return (tool, util.shellquote(toolpath))

    # then merge tools
    tools = {}
    disabled = set()
    for k, v in ui.configitems("merge-tools"):
        t = k.split('.')[0]
        if t not in tools:
            tools[t] = int(_toolstr(ui, t, "priority", "0"))
        if _toolbool(ui, t, "disabled", False):
            disabled.add(t)
    names = tools.keys()
    tools = sorted([(-p, t) for t, p in tools.items() if t not in disabled])
    uimerge = ui.config("ui", "merge")
    if uimerge:
        # external tools defined in uimerge won't be able to handle
        # change/delete conflicts
        if uimerge not in names and not changedelete:
            return (uimerge, uimerge)
        tools.insert(0, (None, uimerge)) # highest priority
    tools.append((None, "hgmerge")) # the old default, if found
    for p, t in tools:
        if check(t, None, symlink, binary, changedelete):
            toolpath = _findtool(ui, t)
            return (t, util.shellquote(toolpath))

    # internal merge or prompt as last resort
    if symlink or binary or changedelete:
        return ":prompt", None
    return ":merge", None

def _eoltype(data):
    "Guess the EOL type of a file"
    if '\0' in data: # binary
        return None
    if '\r\n' in data: # Windows
        return '\r\n'
    if '\r' in data: # Old Mac
        return '\r'
    if '\n' in data: # UNIX
        return '\n'
    return None # unknown

def _matcheol(file, origfile):
    "Convert EOL markers in a file to match origfile"
    tostyle = _eoltype(util.readfile(origfile))
    if tostyle:
        data = util.readfile(file)
        style = _eoltype(data)
        if style:
            newdata = data.replace(style, tostyle)
            if newdata != data:
                util.writefile(file, newdata)

@internaltool('prompt', nomerge)
def _iprompt(repo, mynode, orig, fcd, fco, fca, toolconf):
    """Asks the user which of the local `p1()` or the other `p2()` version to
    keep as the merged version."""
    ui = repo.ui
    fd = fcd.path()

    try:
        if fco.isabsent():
            index = ui.promptchoice(
                _("local changed %s which remote deleted\n"
                  "use (c)hanged version, (d)elete, or leave (u)nresolved?"
                  "$$ &Changed $$ &Delete $$ &Unresolved") % fd, 2)
            choice = ['local', 'other', 'unresolved'][index]
        elif fcd.isabsent():
            index = ui.promptchoice(
                _("remote changed %s which local deleted\n"
                  "use (c)hanged version, leave (d)eleted, or "
                  "leave (u)nresolved?"
                  "$$ &Changed $$ &Deleted $$ &Unresolved") % fd, 2)
            choice = ['other', 'local', 'unresolved'][index]
        else:
            index = ui.promptchoice(
                _("no tool found to merge %s\n"
                  "keep (l)ocal, take (o)ther, or leave (u)nresolved?"
                  "$$ &Local $$ &Other $$ &Unresolved") % fd, 2)
            choice = ['local', 'other', 'unresolved'][index]

        if choice == 'other':
            return _iother(repo, mynode, orig, fcd, fco, fca, toolconf)
        elif choice == 'local':
            return _ilocal(repo, mynode, orig, fcd, fco, fca, toolconf)
        elif choice == 'unresolved':
            return _ifail(repo, mynode, orig, fcd, fco, fca, toolconf)
    except error.ResponseExpected:
        ui.write("\n")
        return _ifail(repo, mynode, orig, fcd, fco, fca, toolconf)

@internaltool('local', nomerge)
def _ilocal(repo, mynode, orig, fcd, fco, fca, toolconf):
    """Uses the local `p1()` version of files as the merged version."""
    return 0, fcd.isabsent()

@internaltool('other', nomerge)
def _iother(repo, mynode, orig, fcd, fco, fca, toolconf):
    """Uses the other `p2()` version of files as the merged version."""
    if fco.isabsent():
        # local changed, remote deleted -- 'deleted' picked
        repo.wvfs.unlinkpath(fcd.path())
        deleted = True
    else:
        repo.wwrite(fcd.path(), fco.data(), fco.flags())
        deleted = False
    return 0, deleted

@internaltool('fail', nomerge)
def _ifail(repo, mynode, orig, fcd, fco, fca, toolconf):
    """
    Rather than attempting to merge files that were modified on both
    branches, it marks them as unresolved. The resolve command must be
    used to resolve these conflicts."""
    # for change/delete conflicts write out the changed version, then fail
    if fcd.isabsent():
        repo.wwrite(fcd.path(), fco.data(), fco.flags())
    return 1, False

def _premerge(repo, fcd, fco, fca, toolconf, files, labels=None):
    tool, toolpath, binary, symlink = toolconf
    if symlink or fcd.isabsent() or fco.isabsent():
        return 1
    a, b, c, back = files

    ui = repo.ui

    validkeep = ['keep', 'keep-merge3']

    # do we attempt to simplemerge first?
    try:
        premerge = _toolbool(ui, tool, "premerge", not binary)
    except error.ConfigError:
        premerge = _toolstr(ui, tool, "premerge").lower()
        if premerge not in validkeep:
            _valid = ', '.join(["'" + v + "'" for v in validkeep])
            raise error.ConfigError(_("%s.premerge not valid "
                                      "('%s' is neither boolean nor %s)") %
                                    (tool, premerge, _valid))

    if premerge:
        if premerge == 'keep-merge3':
            if not labels:
                labels = _defaultconflictlabels
            if len(labels) < 3:
                labels.append('base')
        r = simplemerge.simplemerge(ui, a, b, c, quiet=True, label=labels)
        if not r:
            ui.debug(" premerge successful\n")
            return 0
        if premerge not in validkeep:
            util.copyfile(back, a) # restore from backup and try again
    return 1 # continue merging

def _mergecheck(repo, mynode, orig, fcd, fco, fca, toolconf):
    tool, toolpath, binary, symlink = toolconf
    if symlink:
        repo.ui.warn(_('warning: internal %s cannot merge symlinks '
                       'for %s\n') % (tool, fcd.path()))
        return False
    if fcd.isabsent() or fco.isabsent():
        repo.ui.warn(_('warning: internal %s cannot merge change/delete '
                       'conflict for %s\n') % (tool, fcd.path()))
        return False
    return True

def _merge(repo, mynode, orig, fcd, fco, fca, toolconf, files, labels, mode):
    """
    Uses the internal non-interactive simple merge algorithm for merging
    files. It will fail if there are any conflicts and leave markers in
    the partially merged file. Markers will have two sections, one for each side
    of merge, unless mode equals 'union' which suppresses the markers."""
    a, b, c, back = files

    ui = repo.ui

    r = simplemerge.simplemerge(ui, a, b, c, label=labels, mode=mode)
    return True, r, False

@internaltool('union', fullmerge,
              _("warning: conflicts while merging %s! "
                "(edit, then use 'hg resolve --mark')\n"),
              precheck=_mergecheck)
def _iunion(repo, mynode, orig, fcd, fco, fca, toolconf, files, labels=None):
    """
    Uses the internal non-interactive simple merge algorithm for merging
    files. It will use both left and right sides for conflict regions.
    No markers are inserted."""
    return _merge(repo, mynode, orig, fcd, fco, fca, toolconf,
                  files, labels, 'union')

@internaltool('merge', fullmerge,
              _("warning: conflicts while merging %s! "
                "(edit, then use 'hg resolve --mark')\n"),
              precheck=_mergecheck)
def _imerge(repo, mynode, orig, fcd, fco, fca, toolconf, files, labels=None):
    """
    Uses the internal non-interactive simple merge algorithm for merging
    files. It will fail if there are any conflicts and leave markers in
    the partially merged file. Markers will have two sections, one for each side
    of merge."""
    return _merge(repo, mynode, orig, fcd, fco, fca, toolconf,
                  files, labels, 'merge')

@internaltool('merge3', fullmerge,
              _("warning: conflicts while merging %s! "
                "(edit, then use 'hg resolve --mark')\n"),
              precheck=_mergecheck)
def _imerge3(repo, mynode, orig, fcd, fco, fca, toolconf, files, labels=None):
    """
    Uses the internal non-interactive simple merge algorithm for merging
    files. It will fail if there are any conflicts and leave markers in
    the partially merged file. Marker will have three sections, one from each
    side of the merge and one for the base content."""
    if not labels:
        labels = _defaultconflictlabels
    if len(labels) < 3:
        labels.append('base')
    return _imerge(repo, mynode, orig, fcd, fco, fca, toolconf, files, labels)

def _imergeauto(repo, mynode, orig, fcd, fco, fca, toolconf, files,
                labels=None, localorother=None):
    """
    Generic driver for _imergelocal and _imergeother
    """
    assert localorother is not None
    tool, toolpath, binary, symlink = toolconf
    a, b, c, back = files
    r = simplemerge.simplemerge(repo.ui, a, b, c, label=labels,
                                localorother=localorother)
    return True, r

@internaltool('merge-local', mergeonly, precheck=_mergecheck)
def _imergelocal(*args, **kwargs):
    """
    Like :merge, but resolve all conflicts non-interactively in favor
    of the local `p1()` changes."""
    success, status = _imergeauto(localorother='local', *args, **kwargs)
    return success, status, False

@internaltool('merge-other', mergeonly, precheck=_mergecheck)
def _imergeother(*args, **kwargs):
    """
    Like :merge, but resolve all conflicts non-interactively in favor
    of the other `p2()` changes."""
    success, status = _imergeauto(localorother='other', *args, **kwargs)
    return success, status, False

@internaltool('tagmerge', mergeonly,
              _("automatic tag merging of %s failed! "
                "(use 'hg resolve --tool :merge' or another merge "
                "tool of your choice)\n"))
def _itagmerge(repo, mynode, orig, fcd, fco, fca, toolconf, files, labels=None):
    """
    Uses the internal tag merge algorithm (experimental).
    """
    success, status = tagmerge.merge(repo, fcd, fco, fca)
    return success, status, False

@internaltool('dump', fullmerge)
def _idump(repo, mynode, orig, fcd, fco, fca, toolconf, files, labels=None):
    """
    Creates three versions of the files to merge, containing the
    contents of local, other and base. These files can then be used to
    perform a merge manually. If the file to be merged is named
    ``a.txt``, these files will accordingly be named ``a.txt.local``,
    ``a.txt.other`` and ``a.txt.base`` and they will be placed in the
    same directory as ``a.txt``."""
    a, b, c, back = files

    fd = fcd.path()

    util.copyfile(a, a + ".local")
    repo.wwrite(fd + ".other", fco.data(), fco.flags())
    repo.wwrite(fd + ".base", fca.data(), fca.flags())
    return False, 1, False

def _xmerge(repo, mynode, orig, fcd, fco, fca, toolconf, files, labels=None):
    tool, toolpath, binary, symlink = toolconf
    if fcd.isabsent() or fco.isabsent():
        repo.ui.warn(_('warning: %s cannot merge change/delete conflict '
                       'for %s\n') % (tool, fcd.path()))
        return False, 1, None
    a, b, c, back = files
    out = ""
    env = {'HG_FILE': fcd.path(),
           'HG_MY_NODE': short(mynode),
           'HG_OTHER_NODE': str(fco.changectx()),
           'HG_BASE_NODE': str(fca.changectx()),
           'HG_MY_ISLINK': 'l' in fcd.flags(),
           'HG_OTHER_ISLINK': 'l' in fco.flags(),
           'HG_BASE_ISLINK': 'l' in fca.flags(),
           }

    ui = repo.ui

    args = _toolstr(ui, tool, "args", '$local $base $other')
    if "$output" in args:
        out, a = a, back # read input from backup, write to original
    replace = {'local': a, 'base': b, 'other': c, 'output': out}
    args = util.interpolate(r'\$', replace, args,
                            lambda s: util.shellquote(util.localpath(s)))
    cmd = toolpath + ' ' + args
    repo.ui.debug('launching merge tool: %s\n' % cmd)
    r = ui.system(cmd, cwd=repo.root, environ=env)
    repo.ui.debug('merge tool returned: %s\n' % r)
    return True, r, False

def _formatconflictmarker(repo, ctx, template, label, pad):
    """Applies the given template to the ctx, prefixed by the label.

    Pad is the minimum width of the label prefix, so that multiple markers
    can have aligned templated parts.
    """
    if ctx.node() is None:
        ctx = ctx.p1()

    props = templatekw.keywords.copy()
    props['templ'] = template
    props['ctx'] = ctx
    props['repo'] = repo
    templateresult = template('conflictmarker', **props)

    label = ('%s:' % label).ljust(pad + 1)
    mark = '%s %s' % (label, templater.stringify(templateresult))

    if mark:
        mark = mark.splitlines()[0] # split for safety

    # 8 for the prefix of conflict marker lines (e.g. '<<<<<<< ')
    return util.ellipsis(mark, 80 - 8)

_defaultconflictmarker = ('{node|short} ' +
    '{ifeq(tags, "tip", "", "{tags} ")}' +
    '{if(bookmarks, "{bookmarks} ")}' +
    '{ifeq(branch, "default", "", "{branch} ")}' +
    '- {author|user}: {desc|firstline}')

_defaultconflictlabels = ['local', 'other']

def _formatlabels(repo, fcd, fco, fca, labels):
    """Formats the given labels using the conflict marker template.

    Returns a list of formatted labels.
    """
    cd = fcd.changectx()
    co = fco.changectx()
    ca = fca.changectx()

    ui = repo.ui
    template = ui.config('ui', 'mergemarkertemplate', _defaultconflictmarker)
    tmpl = formatter.maketemplater(ui, 'conflictmarker', template)

    pad = max(len(l) for l in labels)

    newlabels = [_formatconflictmarker(repo, cd, tmpl, labels[0], pad),
                 _formatconflictmarker(repo, co, tmpl, labels[1], pad)]
    if len(labels) > 2:
        newlabels.append(_formatconflictmarker(repo, ca, tmpl, labels[2], pad))
    return newlabels

def _filemerge(premerge, repo, mynode, orig, fcd, fco, fca, labels=None):
    """perform a 3-way merge in the working directory

    premerge = whether this is a premerge
    mynode = parent node before merge
    orig = original local filename before merge
    fco = other file context
    fca = ancestor file context
    fcd = local file context for current/destination file

    Returns whether the merge is complete, the return value of the merge, and
    a boolean indicating whether the file was deleted from disk."""

    def temp(prefix, ctx):
        pre = "%s~%s." % (os.path.basename(ctx.path()), prefix)
        (fd, name) = tempfile.mkstemp(prefix=pre)
        data = repo.wwritedata(ctx.path(), ctx.data())
        f = os.fdopen(fd, "wb")
        f.write(data)
        f.close()
        return name

    if not fco.cmp(fcd): # files identical?
        return True, None, False

    ui = repo.ui
    fd = fcd.path()
    binary = fcd.isbinary() or fco.isbinary() or fca.isbinary()
    symlink = 'l' in fcd.flags() + fco.flags()
    changedelete = fcd.isabsent() or fco.isabsent()
    tool, toolpath = _picktool(repo, ui, fd, binary, symlink, changedelete)
    if tool in internals and tool.startswith('internal:'):
        # normalize to new-style names (':merge' etc)
        tool = tool[len('internal'):]
    ui.debug("picked tool '%s' for %s (binary %s symlink %s changedelete %s)\n"
             % (tool, fd, binary, symlink, changedelete))

    if tool in internals:
        func = internals[tool]
        mergetype = func.mergetype
        onfailure = func.onfailure
        precheck = func.precheck
    else:
        func = _xmerge
        mergetype = fullmerge
        onfailure = _("merging %s failed!\n")
        precheck = None

    toolconf = tool, toolpath, binary, symlink

    if mergetype == nomerge:
        r, deleted = func(repo, mynode, orig, fcd, fco, fca, toolconf)
        return True, r, deleted

    if premerge:
        if orig != fco.path():
            ui.status(_("merging %s and %s to %s\n") % (orig, fco.path(), fd))
        else:
            ui.status(_("merging %s\n") % fd)

    ui.debug("my %s other %s ancestor %s\n" % (fcd, fco, fca))

    if precheck and not precheck(repo, mynode, orig, fcd, fco, fca,
                                 toolconf):
        if onfailure:
            ui.warn(onfailure % fd)
        return True, 1, False

    a = repo.wjoin(fd)
    b = temp("base", fca)
    c = temp("other", fco)
    if not fcd.isabsent():
        back = scmutil.origpath(ui, repo, a)
        if premerge:
            util.copyfile(a, back)
    else:
        back = None
    files = (a, b, c, back)

    r = 1
    try:
        markerstyle = ui.config('ui', 'mergemarkers', 'basic')
        if not labels:
            labels = _defaultconflictlabels
        if markerstyle != 'basic':
            labels = _formatlabels(repo, fcd, fco, fca, labels)

        if premerge and mergetype == fullmerge:
            r = _premerge(repo, fcd, fco, fca, toolconf, files, labels=labels)
            # complete if premerge successful (r is 0)
            return not r, r, False

        needcheck, r, deleted = func(repo, mynode, orig, fcd, fco, fca,
                                     toolconf, files, labels=labels)

        if needcheck:
            r = _check(r, ui, tool, fcd, files)

        if r:
            if onfailure:
                ui.warn(onfailure % fd)

        return True, r, deleted
    finally:
        if not r and back is not None:
            util.unlink(back)
        util.unlink(b)
        util.unlink(c)

def _check(r, ui, tool, fcd, files):
    fd = fcd.path()
    a, b, c, back = files

    if not r and (_toolbool(ui, tool, "checkconflicts") or
                  'conflicts' in _toollist(ui, tool, "check")):
        if re.search("^(<<<<<<< .*|=======|>>>>>>> .*)$", fcd.data(),
                     re.MULTILINE):
            r = 1

    checked = False
    if 'prompt' in _toollist(ui, tool, "check"):
        checked = True
        if ui.promptchoice(_("was merge of '%s' successful (yn)?"
                             "$$ &Yes $$ &No") % fd, 1):
            r = 1

    if not r and not checked and (_toolbool(ui, tool, "checkchanged") or
                                  'changed' in
                                  _toollist(ui, tool, "check")):
        if back is not None and filecmp.cmp(a, back):
            if ui.promptchoice(_(" output file %s appears unchanged\n"
                                 "was merge successful (yn)?"
                                 "$$ &Yes $$ &No") % fd, 1):
                r = 1

    if back is not None and _toolbool(ui, tool, "fixeol"):
        _matcheol(a, back)

    return r

def premerge(repo, mynode, orig, fcd, fco, fca, labels=None):
    return _filemerge(True, repo, mynode, orig, fcd, fco, fca, labels=labels)

def filemerge(repo, mynode, orig, fcd, fco, fca, labels=None):
    return _filemerge(False, repo, mynode, orig, fcd, fco, fca, labels=labels)

# tell hggettext to extract docstrings from these functions:
i18nfunctions = internals.values()
