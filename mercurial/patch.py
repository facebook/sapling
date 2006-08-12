# patch.py - patch file parsing routines
#
# Copyright 2006 Brendan Cully <brendan@kublai.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from demandload import demandload
demandload(globals(), "util")
demandload(globals(), "os re shutil tempfile")

def readgitpatch(patchname):
    """extract git-style metadata about patches from <patchname>"""
    class gitpatch:
        "op is one of ADD, DELETE, RENAME, MODIFY or COPY"
        def __init__(self, path):
            self.path = path
            self.oldpath = None
            self.mode = None
            self.op = 'MODIFY'
            self.copymod = False
            self.lineno = 0
    
    # Filter patch for git information
    gitre = re.compile('diff --git a/(.*) b/(.*)')
    pf = file(patchname)
    gp = None
    gitpatches = []
    # Can have a git patch with only metadata, causing patch to complain
    dopatch = False

    lineno = 0
    for line in pf:
        lineno += 1
        if line.startswith('diff --git'):
            m = gitre.match(line)
            if m:
                if gp:
                    gitpatches.append(gp)
                src, dst = m.group(1,2)
                gp = gitpatch(dst)
                gp.lineno = lineno
        elif gp:
            if line.startswith('--- '):
                if gp.op in ('COPY', 'RENAME'):
                    gp.copymod = True
                    dopatch = 'filter'
                gitpatches.append(gp)
                gp = None
                if not dopatch:
                    dopatch = True
                continue
            if line.startswith('rename from '):
                gp.op = 'RENAME'
                gp.oldpath = line[12:].rstrip()
            elif line.startswith('rename to '):
                gp.path = line[10:].rstrip()
            elif line.startswith('copy from '):
                gp.op = 'COPY'
                gp.oldpath = line[10:].rstrip()
            elif line.startswith('copy to '):
                gp.path = line[8:].rstrip()
            elif line.startswith('deleted file'):
                gp.op = 'DELETE'
            elif line.startswith('new file mode '):
                gp.op = 'ADD'
                gp.mode = int(line.rstrip()[-3:], 8)
            elif line.startswith('new mode '):
                gp.mode = int(line.rstrip()[-3:], 8)
    if gp:
        gitpatches.append(gp)

    if not gitpatches:
        dopatch = True

    return (dopatch, gitpatches)

def dogitpatch(patchname, gitpatches):
    """Preprocess git patch so that vanilla patch can handle it"""
    pf = file(patchname)
    pfline = 1

    fd, patchname = tempfile.mkstemp(prefix='hg-patch-')
    tmpfp = os.fdopen(fd, 'w')

    try:
        for i in range(len(gitpatches)):
            p = gitpatches[i]
            if not p.copymod:
                continue

            if os.path.exists(p.path):
                raise util.Abort(_("cannot create %s: destination already exists") %
                            p.path)

            (src, dst) = [os.path.join(os.getcwd(), n)
                          for n in (p.oldpath, p.path)]

            targetdir = os.path.dirname(dst)
            if not os.path.isdir(targetdir):
                os.makedirs(targetdir)
            try:
                shutil.copyfile(src, dst)
                shutil.copymode(src, dst)
            except shutil.Error, inst:
                raise util.Abort(str(inst))

            # rewrite patch hunk
            while pfline < p.lineno:
                tmpfp.write(pf.readline())
                pfline += 1
            tmpfp.write('diff --git a/%s b/%s\n' % (p.path, p.path))
            line = pf.readline()
            pfline += 1
            while not line.startswith('--- a/'):
                tmpfp.write(line)
                line = pf.readline()
                pfline += 1
            tmpfp.write('--- a/%s\n' % p.path)

        line = pf.readline()
        while line:
            tmpfp.write(line)
            line = pf.readline()
    except:
        tmpfp.close()
        os.unlink(patchname)
        raise

    tmpfp.close()
    return patchname

def patch(strip, patchname, ui, cwd=None):
    """apply the patch <patchname> to the working directory.
    a list of patched files is returned"""

    (dopatch, gitpatches) = readgitpatch(patchname)

    files = {}
    if dopatch:
        if dopatch == 'filter':
            patchname = dogitpatch(patchname, gitpatches)
        patcher = util.find_in_path('gpatch', os.environ.get('PATH', ''), 'patch')
        args = []
        if cwd:
            args.append('-d %s' % util.shellquote(cwd))
        fp = os.popen('%s %s -p%d < %s' % (patcher, ' '.join(args), strip,
                                           util.shellquote(patchname)))

        if dopatch == 'filter':
            False and os.unlink(patchname)

        for line in fp:
            line = line.rstrip()
            ui.status("%s\n" % line)
            if line.startswith('patching file '):
                pf = util.parse_patch_output(line)
                files.setdefault(pf, (None, None))
        code = fp.close()
        if code:
            raise util.Abort(_("patch command failed: %s") % explain_exit(code)[0])

    for gp in gitpatches:
        files[gp.path] = (gp.op, gp)

    return files
