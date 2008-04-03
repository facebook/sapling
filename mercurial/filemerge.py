# filemerge.py - file-level merge handling for Mercurial
#
# Copyright 2006, 2007, 2008 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from node import nullrev
from i18n import _
import util, os, tempfile, simplemerge, re, filecmp

def _toolstr(ui, tool, part, default=""):
    return ui.config("merge-tools", tool + "." + part, default)

def _toolbool(ui, tool, part, default=False):
    return ui.configbool("merge-tools", tool + "." + part, default)

def _findtool(ui, tool):
    if tool in ("internal:fail", "internal:local", "internal:other"):
        return tool
    k = _toolstr(ui, tool, "regkey")
    if k:
        p = util.lookup_reg(k, _toolstr(ui, tool, "regname"))
        if p:
            p = util.find_exe(p + _toolstr(ui, tool, "regappend"))
            if p:
                return p
    return util.find_exe(_toolstr(ui, tool, "executable", tool))

def _picktool(repo, ui, path, binary, symlink):
    def check(tool, pat, symlink, binary):
        tmsg = tool
        if pat:
            tmsg += " specified for " + pat
        if pat and not _findtool(ui, tool): # skip search if not matching
            ui.warn(_("couldn't find merge tool %s\n") % tmsg)
        elif symlink and not _toolbool(ui, tool, "symlink"):
            ui.warn(_("tool %s can't handle symlinks\n") % tmsg)
        elif binary and not _toolbool(ui, tool, "binary"):
            ui.warn(_("tool %s can't handle binary\n") % tmsg)
        elif not util.gui() and _toolbool(ui, tool, "gui"):
            ui.warn(_("tool %s requires a GUI\n") % tmsg)
        else:
            return True
        return False

    # HGMERGE takes precedence
    hgmerge = os.environ.get("HGMERGE")
    if hgmerge:
        return (hgmerge, hgmerge)

    # then patterns
    for pat, tool in ui.configitems("merge-patterns"):
        mf = util.matcher(repo.root, "", [pat], [], [])[1]
        if mf(path) and check(tool, pat, symlink, False):
                toolpath = _findtool(ui, tool)
                return (tool, '"' + toolpath + '"')

    # then merge tools
    tools = {}
    for k,v in ui.configitems("merge-tools"):
        t = k.split('.')[0]
        if t not in tools:
            tools[t] = int(_toolstr(ui, t, "priority", "0"))
    names = tools.keys()
    tools = [(-p,t) for t,p in tools.items()]
    tools.sort()
    uimerge = ui.config("ui", "merge")
    if uimerge:
        if uimerge not in names:
            return (uimerge, uimerge)
        tools.insert(0, (None, uimerge)) # highest priority
    tools.append((None, "hgmerge")) # the old default, if found
    for p,t in tools:
        toolpath = _findtool(ui, t)
        if toolpath and check(t, None, symlink, binary):
            return (t, '"' + toolpath + '"')
    # internal merge as last resort
    return (not (symlink or binary) and "internal:merge" or None, None)

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
    tostyle = _eoltype(open(origfile, "rb").read())
    if tostyle:
        data = open(file, "rb").read()
        style = _eoltype(data)
        if style:
            newdata = data.replace(style, tostyle)
            if newdata != data:
                open(file, "wb").write(newdata)

def filemerge(repo, fw, fd, fo, wctx, mctx):
    """perform a 3-way merge in the working directory

    fw = original filename in the working directory
    fd = destination filename in the working directory
    fo = filename in other parent
    wctx, mctx = working and merge changecontexts
    """

    def temp(prefix, ctx):
        pre = "%s~%s." % (os.path.basename(ctx.path()), prefix)
        (fd, name) = tempfile.mkstemp(prefix=pre)
        data = repo.wwritedata(ctx.path(), ctx.data())
        f = os.fdopen(fd, "wb")
        f.write(data)
        f.close()
        return name

    def isbin(ctx):
        try:
            return util.binary(ctx.data())
        except IOError:
            return False

    fco = mctx.filectx(fo)
    if not fco.cmp(wctx.filectx(fd).data()): # files identical?
        return None

    ui = repo.ui
    fcm = wctx.filectx(fw)
    fca = fcm.ancestor(fco) or repo.filectx(fw, fileid=nullrev)
    binary = isbin(fcm) or isbin(fco) or isbin(fca)
    symlink = fcm.islink() or fco.islink()
    tool, toolpath = _picktool(repo, ui, fw, binary, symlink)
    ui.debug(_("picked tool '%s' for %s (binary %s symlink %s)\n") %
               (tool, fw, binary, symlink))

    if not tool:
        tool = "internal:local"
        if ui.prompt(_(" no tool found to merge %s\n"
                       "keep (l)ocal or take (o)ther?") % fw,
                     _("[lo]"), _("l")) != _("l"):
            tool = "internal:other"
    if tool == "internal:local":
        return 0
    if tool == "internal:other":
        repo.wwrite(fd, fco.data(), fco.fileflags())
        return 0
    if tool == "internal:fail":
        return 1

    # do the actual merge
    a = repo.wjoin(fd)
    b = temp("base", fca)
    c = temp("other", fco)
    out = ""
    back = a + ".orig"
    util.copyfile(a, back)

    if fw != fo:
        repo.ui.status(_("merging %s and %s\n") % (fw, fo))
    else:
        repo.ui.status(_("merging %s\n") % fw)
    repo.ui.debug(_("my %s other %s ancestor %s\n") % (fcm, fco, fca))

    # do we attempt to simplemerge first?
    if _toolbool(ui, tool, "premerge", not (binary or symlink)):
        r = simplemerge.simplemerge(a, b, c, quiet=True)
        if not r:
            ui.debug(_(" premerge successful\n"))
            os.unlink(back)
            os.unlink(b)
            os.unlink(c)
            return 0
        util.copyfile(back, a) # restore from backup and try again

    env = dict(HG_FILE=fd,
               HG_MY_NODE=str(wctx.parents()[0]),
               HG_OTHER_NODE=str(mctx),
               HG_MY_ISLINK=fcm.islink(),
               HG_OTHER_ISLINK=fco.islink(),
               HG_BASE_ISLINK=fca.islink())

    if tool == "internal:merge":
        r = simplemerge.simplemerge(a, b, c, label=['local', 'other'])
    else:
        args = _toolstr(ui, tool, "args", '$local $base $other')
        if "$output" in args:
            out, a = a, back # read input from backup, write to original
        replace = dict(local=a, base=b, other=c, output=out)
        args = re.sub("\$(local|base|other|output)",
                      lambda x: '"%s"' % replace[x.group()[1:]], args)
        r = util.system(toolpath + ' ' + args, cwd=repo.root, environ=env)

    if not r and _toolbool(ui, tool, "checkconflicts"):
        if re.match("^(<<<<<<< .*|=======|>>>>>>> .*)$", fcm.data()):
            r = 1

    if not r and _toolbool(ui, tool, "checkchanged"):
        if filecmp.cmp(repo.wjoin(fd), back):
            if ui.prompt(_(" output file %s appears unchanged\n"
                "was merge successful (yn)?") % fd,
                _("[yn]"), _("n")) != _("y"):
                r = 1

    if _toolbool(ui, tool, "fixeol"):
        _matcheol(repo.wjoin(fd), back)

    if r:
        repo.ui.warn(_("merging %s failed!\n") % fd)
    else:
        os.unlink(back)

    os.unlink(b)
    os.unlink(c)
    return r
