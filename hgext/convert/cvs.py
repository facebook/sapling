# CVS conversion code inspired by hg-cvs-import and git-cvsimport

import os, locale, re, socket
from mercurial import util

from common import NoRepo, commit, converter_source

class convert_cvs(converter_source):
    def __init__(self, ui, path):
        self.path = path
        self.ui = ui
        cvs = os.path.join(path, "CVS")
        if not os.path.exists(cvs):
            raise NoRepo("couldn't open CVS repo %s" % path)

        self.changeset = {}
        self.files = {}
        self.tags = {}
        self.lastbranch = {}
        self.parent = {}
        self.socket = None
        self.cvsroot = file(os.path.join(cvs, "Root")).read()[:-1]
        self.cvsrepo = file(os.path.join(cvs, "Repository")).read()[:-1]
        self.encoding = locale.getpreferredencoding()
        self._parse()
        self._connect()

    def _parse(self):
        if self.changeset:
            return

        d = os.getcwd()
        try:
            os.chdir(self.path)
            id = None
            state = 0
            for l in os.popen("cvsps -A -u --cvs-direct -q"):
                if state == 0: # header
                    if l.startswith("PatchSet"):
                        id = l[9:-2]
                    elif l.startswith("Date"):
                        date = util.parsedate(l[6:-1], ["%Y/%m/%d %H:%M:%S"])
                        date = util.datestr(date)
                    elif l.startswith("Branch"):
                        branch = l[8:-1]
                        self.parent[id] = self.lastbranch.get(branch, 'bad')
                        self.lastbranch[branch] = id
                    elif l.startswith("Ancestor branch"):
                        ancestor = l[17:-1]
                        self.parent[id] = self.lastbranch[ancestor]
                    elif l.startswith("Author"):
                        author = self.recode(l[8:-1])
                    elif l.startswith("Tag:") or l.startswith("Tags:"):
                        t = l[l.index(':')+1:]
                        t = [ut.strip() for ut in t.split(',')]
                        if (len(t) > 1) or (t[0] and (t[0] != "(none)")):
                            self.tags.update(dict.fromkeys(t, id))
                    elif l.startswith("Log:"):
                        state = 1
                        log = ""
                elif state == 1: # log
                    if l == "Members: \n":
                        files = {}
                        log = self.recode(log[:-1])
                        if log.isspace():
                            log = "*** empty log message ***\n"
                        state = 2
                    else:
                        log += l
                elif state == 2:
                    if l == "\n": #
                        state = 0
                        p = [self.parent[id]]
                        if id == "1":
                            p = []
                        if branch == "HEAD":
                            branch = ""
                        c = commit(author=author, date=date, parents=p,
                                   desc=log, branch=branch)
                        self.changeset[id] = c
                        self.files[id] = files
                    else:
                        colon = l.rfind(':')
                        file = l[1:colon]
                        rev = l[colon+1:-2]
                        rev = rev.split("->")[1]
                        files[file] = rev

            self.heads = self.lastbranch.values()
        finally:
            os.chdir(d)

    def _connect(self):
        root = self.cvsroot
        conntype = None
        user, host = None, None
        cmd = ['cvs', 'server']

        self.ui.status("connecting to %s\n" % root)

        if root.startswith(":pserver:"):
            root = root[9:]
            m = re.match(r'(?:(.*?)(?::(.*?))?@)?([^:\/]*)(?::(\d*))?(.*)',
                         root)
            if m:
                conntype = "pserver"
                user, passw, serv, port, root = m.groups()
                if not user:
                    user = "anonymous"
                rr = ":pserver:" + user + "@" + serv + ":" +  root
                if port:
                    rr2, port = "-", int(port)
                else:
                    rr2, port = rr, 2401
                rr += str(port)

                if not passw:
                    passw = "A"
                    pf = open(os.path.join(os.environ["HOME"], ".cvspass"))
                    for l in pf:
                        # :pserver:cvs@mea.tmt.tele.fi:/cvsroot/zmailer Ah<Z
                        m = re.match(r'(/\d+\s+/)?(.*)', l)
                        l = m.group(2)
                        w, p = l.split(' ', 1)
                        if w in [rr, rr2]:
                            passw = p
                            break
                    pf.close()

                sck = socket.socket()
                sck.connect((serv, port))
                sck.send("\n".join(["BEGIN AUTH REQUEST", root, user, passw,
                                    "END AUTH REQUEST", ""]))
                if sck.recv(128) != "I LOVE YOU\n":
                    raise NoRepo("CVS pserver authentication failed")

                self.writep = self.readp = sck.makefile('r+')

        if not conntype and root.startswith(":local:"):
            conntype = "local"
            root = root[7:]

        if not conntype:
            # :ext:user@host/home/user/path/to/cvsroot
            if root.startswith(":ext:"):
                root = root[5:]
            m = re.match(r'(?:([^@:/]+)@)?([^:/]+):?(.*)', root)
            if not m:
                conntype = "local"
            else:
                conntype = "rsh"
                user, host, root = m.group(1), m.group(2), m.group(3)

        if conntype != "pserver":
            if conntype == "rsh":
                rsh = os.environ.get("CVS_RSH" or "rsh")
                if user:
                    cmd = [rsh, '-l', user, host] + cmd
                else:
                    cmd = [rsh, host] + cmd

            self.writep, self.readp = os.popen2(cmd)

        self.realroot = root

        self.writep.write("Root %s\n" % root)
        self.writep.write("Valid-responses ok error Valid-requests Mode"
                          " M Mbinary E Checked-in Created Updated"
                          " Merged Removed\n")
        self.writep.write("valid-requests\n")
        self.writep.flush()
        r = self.readp.readline()
        if not r.startswith("Valid-requests"):
            raise util.Abort("server sucks")
        if "UseUnchanged" in r:
            self.writep.write("UseUnchanged\n")
            self.writep.flush()
            r = self.readp.readline()

    def getheads(self):
        return self.heads

    def _getfile(self, name, rev):
        if rev.endswith("(DEAD)"):
            raise IOError

        args = ("-N -P -kk -r %s --" % rev).split()
        args.append(os.path.join(self.cvsrepo, name))
        for x in args:
            self.writep.write("Argument %s\n" % x)
        self.writep.write("Directory .\n%s\nco\n" % self.realroot)
        self.writep.flush()

        data = ""
        while 1:
            line = self.readp.readline()
            if line.startswith("Created ") or line.startswith("Updated "):
                self.readp.readline() # path
                self.readp.readline() # entries
                mode = self.readp.readline()[:-1]
                count = int(self.readp.readline()[:-1])
                data = self.readp.read(count)
            elif line.startswith(" "):
                data += line[1:]
            elif line.startswith("M "):
                pass
            elif line.startswith("Mbinary "):
                count = int(self.readp.readline()[:-1])
                data = self.readp.read(count)
            else:
                if line == "ok\n":
                    return (data, "x" in mode and "x" or "")
                elif line.startswith("E "):
                    self.ui.warn("cvs server: %s\n" % line[2:])
                elif line.startswith("Remove"):
                    l = self.readp.readline()
                    l = self.readp.readline()
                    if l != "ok\n":
                        raise util.Abort("unknown CVS response: %s" % l)
                else:
                    raise util.Abort("unknown CVS response: %s" % line)

    def getfile(self, file, rev):
        data, mode = self._getfile(file, rev)
        self.modecache[(file, rev)] = mode
        return data

    def getmode(self, file, rev):
        return self.modecache[(file, rev)]

    def getchanges(self, rev):
        self.modecache = {}
        files = self.files[rev]
        cl = files.items()
        cl.sort()
        return cl

    def recode(self, text):
        return text.decode(self.encoding, "replace").encode("utf-8")

    def getcommit(self, rev):
        return self.changeset[rev]

    def gettags(self):
        return self.tags
