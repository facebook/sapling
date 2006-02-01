import os, tempfile, binascii, errno
from mercurial import util
from mercurial import node as hgnode

class gpg:
    def __init__(self, path, key=None):
        self.path = path
        self.key = (key and " --local-user \"%s\"" % key) or ""

    def sign(self, data):
        gpgcmd = "%s --sign --detach-sign%s" % (self.path, self.key)
        return util.filter(data, gpgcmd)

    def verify(self, data, sig):
        """ returns of the good and bad signatures"""
        try:
            fd, sigfile = tempfile.mkstemp(prefix="hggpgsig")
            fp = os.fdopen(fd, 'wb')
            fp.write(sig)
            fp.close()
            fd, datafile = tempfile.mkstemp(prefix="hggpgdata")
            fp = os.fdopen(fd, 'wb')
            fp.write(data)
            fp.close()
            gpgcmd = "%s --logger-fd 1 --status-fd 1 --verify \"%s\" \"%s\"" % (self.path, sigfile, datafile)
            #gpgcmd = "%s --status-fd 1 --verify \"%s\" \"%s\"" % (self.path, sigfile, datafile)
            ret = util.filter("", gpgcmd)
        except:
            for f in (sigfile, datafile):
                try:
                    if f: os.unlink(f)
                except: pass
            raise
        keys = []
        key, fingerprint = None, None
        err = ""
        for l in ret.splitlines():
            # see DETAILS in the gnupg documentation
            # filter the logger output
            if not l.startswith("[GNUPG:]"):
                continue
            l = l[9:]
            if l.startswith("ERRSIG"):
                err = "error while verifying signature"
                break
            elif l.startswith("VALIDSIG"):
                # fingerprint of the primary key
                fingerprint = l.split()[10]
            elif (l.startswith("GOODSIG") or
                  l.startswith("EXPSIG") or
                  l.startswith("EXPKEYSIG") or
                  l.startswith("BADSIG")):
                if key is not None:
                    keys.append(key + [fingerprint])
                key = l.split(" ", 2)
                fingerprint = None
        if err:
            return err, []
        if key is not None:
            keys.append(key + [fingerprint])
        return err, keys

def newgpg(ui, **opts):
    gpgpath = ui.config("gpg", "cmd", "gpg")
    gpgkey = opts.get('key')
    if not gpgkey:
        gpgkey = ui.config("gpg", "key", None)
    return gpg(gpgpath, gpgkey)

def check(ui, repo, rev):
    """verify all the signatures there may be for a particular revision"""
    mygpg = newgpg(ui)
    rev = repo.lookup(rev)
    hexrev = hgnode.hex(rev)
    keys = []

    def addsig(fn, ln, l):
        if not l: return
        n, v, sig = l.split(" ", 2)
        if n == hexrev:
            data = node2txt(repo, rev, v)
            sig = binascii.a2b_base64(sig)
            err, k = mygpg.verify(data, sig)
            if not err:
                keys.append((k, fn, ln))
            else:
                ui.warn("%s:%d %s\n" % (fn, ln , err))

    fl = repo.file(".hgsigs")
    h = fl.heads()
    h.reverse()
    # read the heads
    for r in h:
        ln = 1
        for l in fl.read(r).splitlines():
            addsig(".hgsigs|%s" % hgnode.short(r), ln, l)
            ln +=1
    try:
        # read local signatures
        ln = 1
        f = repo.opener("localsigs")
        for l in f:
            addsig("localsigs", ln, l)
            ln +=1
    except IOError:
        pass

    if not keys:
        ui.write("%s not signed\n" % hgnode.short(rev))
        return
    valid = []
    # warn for expired key and/or sigs
    for k, fn, ln in keys:
        prefix = "%s:%d" % (fn, ln)
        for key in k:
            if key[0] == "BADSIG":
                ui.write("%s Bad signature from \"%s\"\n" % (prefix, key[2]))
                continue
            if key[0] == "EXPSIG":
                ui.write("%s Note: Signature has expired"
                         " (signed by: \"%s\")\n" % (prefix, key[2]))
            elif key[0] == "EXPKEYSIG":
                ui.write("%s Note: This key has expired"
                         " (signed by: \"%s\")\n" % (prefix, key[2]))
            valid.append((key[1], key[2], key[3]))
    # print summary
    ui.write("%s is signed by:\n" % hgnode.short(rev))
    for keyid, user, fingerprint in valid:
        role = getrole(ui, fingerprint)
        ui.write("  %s (%s)\n" % (user, role))

def getrole(ui, fingerprint):
    return ui.config("gpg", fingerprint, "no role defined")

def sign(ui, repo, *revs, **opts):
    """add a signature for the current tip or a given revision"""
    mygpg = newgpg(ui, **opts)
    sigver = "0"
    sigmessage = ""
    if revs:
        nodes = [repo.lookup(n) for n in revs]
    else:
        nodes = [repo.changelog.tip()]

    for n in nodes:
        hexnode = hgnode.hex(n)
        ui.write("Signing %d:%s\n" % (repo.changelog.rev(n),
                                      hgnode.short(n)))
        # build data
        data = node2txt(repo, n, sigver)
        sig = mygpg.sign(data)
        if not sig:
            raise util.Abort("Error while signing")
        sig = binascii.b2a_base64(sig)
        sig = sig.replace("\n", "")
        sigmessage += "%s %s %s\n" % (hexnode, sigver, sig)

    # write it
    if opts['local']:
        repo.opener("localsigs", "ab").write(sigmessage)
        return

    for x in repo.changes():
        if ".hgsigs" in x and not opts["force"]:
            raise util.Abort("working copy of .hgsigs is changed "
                             "(please commit .hgsigs manually "
                             "or use --force)")

    repo.wfile(".hgsigs", "ab").write(sigmessage)

    if repo.dirstate.state(".hgsigs") == '?':
        repo.add([".hgsigs"])

    if opts["no_commit"]:
        return

    message = opts['message']
    if not message:
        message = "\n".join(["Added signature for changeset %s" % hgnode.hex(n)
                             for n in nodes])
    try:
        repo.commit([".hgsigs"], message, opts['user'], opts['date'])
    except ValueError, inst:
        raise util.Abort(str(inst))

def node2txt(repo, node, ver):
    """map a manifest into some text"""
    if ver == "0":
        return "%s\n" % hgnode.hex(node)
    else:
        util.Abort("unknown signature version")

cmdtable = {
    "sign":
        (sign,
         [('l', 'local', None, "make the signature local"),
          ('f', 'force', None, "sign even if the sigfile is modified"),
          ('', 'no-commit', None, "do not commit the sigfile after signing"),
          ('m', 'message', "", "commit message"),
          ('d', 'date', "", "date code"),
          ('u', 'user', "", "user"),
          ('k', 'key', "", "the key id to sign with")],
         "hg sign [OPTION]... REVISIONS"),
    "sigcheck": (check, [], 'hg sigcheck REVISION')
}

