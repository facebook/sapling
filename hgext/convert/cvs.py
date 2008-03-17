# CVS conversion code inspired by hg-cvs-import and git-cvsimport

import os, locale, re, socket
from cStringIO import StringIO
from mercurial import util

from common import NoRepo, commit, converter_source, checktool

class convert_cvs(converter_source):
    def __init__(self, ui, path, rev=None):
        super(convert_cvs, self).__init__(ui, path, rev=rev)

        cvs = os.path.join(path, "CVS")
        if not os.path.exists(cvs):
            raise NoRepo("%s does not look like a CVS checkout" % path)

        self.cmd = ui.config('convert', 'cvsps', 'cvsps -A -u --cvs-direct -q')
        cvspsexe = self.cmd.split(None, 1)[0]
        for tool in (cvspsexe, 'cvs'):
            checktool(tool)

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

        maxrev = 0
        cmd = self.cmd
        if self.rev:
            # TODO: handle tags
            try:
                # patchset number?
                maxrev = int(self.rev)
            except ValueError:
                try:
                    # date
                    util.parsedate(self.rev, ['%Y/%m/%d %H:%M:%S'])
                    cmd = '%s -d "1970/01/01 00:00:01" -d "%s"' % (cmd, self.rev)
                except util.Abort:
                    raise util.Abort('revision %s is not a patchset number or date' % self.rev)

        d = os.getcwd()
        try:
            os.chdir(self.path)
            id = None
            state = 0
            filerevids = {}
            for l in util.popen(cmd):
                if state == 0: # header
                    if l.startswith("PatchSet"):
                        id = l[9:-2]
                        if maxrev and int(id) > maxrev:
                            # ignore everything
                            state = 3
                    elif l.startswith("Date"):
                        date = util.parsedate(l[6:-1], ["%Y/%m/%d %H:%M:%S"])
                        date = util.datestr(date)
                    elif l.startswith("Branch"):
                        branch = l[8:-1]
                        self.parent[id] = self.lastbranch.get(branch, 'bad')
                        self.lastbranch[branch] = id
                    elif l.startswith("Ancestor branch"):
                        ancestor = l[17:-1]
                        # figure out the parent later
                        self.parent[id] = self.lastbranch[ancestor]
                    elif l.startswith("Author"):
                        author = self.recode(l[8:-1])
                    elif l.startswith("Tag:") or l.startswith("Tags:"):
                        t = l[l.index(':')+1:]
                        t = [ut.strip() for ut in t.split(',')]
                        if (len(t) > 1) or (t[0] and (t[0] != "(none)")):
                            self.tags.update(dict.fromkeys(t, id))
                    elif l.startswith("Log:"):
                        # switch to gathering log
                        state = 1
                        log = ""
                elif state == 1: # log
                    if l == "Members: \n":
                        # switch to gathering members
                        files = {}
                        oldrevs = []
                        log = self.recode(log[:-1])
                        state = 2
                    else:
                        # gather log
                        log += l
                elif state == 2: # members
                    if l == "\n": # start of next entry
                        state = 0
                        p = [self.parent[id]]
                        if id == "1":
                            p = []
                        if branch == "HEAD":
                            branch = ""
                        if branch:
                            latest = None
                            # the last changeset that contains a base
                            # file is our parent
                            for r in oldrevs:
                                latest = max(filerevids.get(r, None), latest)
                            if latest:
                                p = [latest]

                        # add current commit to set
                        c = commit(author=author, date=date, parents=p,
                                   desc=log, branch=branch)
                        self.changeset[id] = c
                        self.files[id] = files
                    else:
                        colon = l.rfind(':')
                        file = l[1:colon]
                        rev = l[colon+1:-2]
                        oldrev, rev = rev.split("->")
                        files[file] = rev

                        # save some information for identifying branch points
                        oldrevs.append("%s:%s" % (oldrev, file))
                        filerevids["%s:%s" % (rev, file)] = id
                elif state == 3:
                    # swallow all input
                    continue

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
                if not port:
                    port = 2401
                else:
                    port = int(port)
                format0 = ":pserver:%s@%s:%s" % (user, serv, root)
                format1 = ":pserver:%s@%s:%d%s" % (user, serv, port, root)

                if not passw:
                    passw = "A"
                    pf = open(os.path.join(os.environ["HOME"], ".cvspass"))
                    for line in pf.read().splitlines():
                        part1, part2 = line.split(' ', 1)
                        if part1 == '/1':
                            # /1 :pserver:user@example.com:2401/cvsroot/foo Ah<Z
                            part1, part2 = part2.split(' ', 1)
                            format = format1
                        else:
                            # :pserver:user@example.com:/cvsroot/foo Ah<Z
                            format = format0
                        if part1 == format:
                            passw = part2
                            break
                    pf.close()

                sck = socket.socket()
                sck.connect((serv, port))
                sck.send("\n".join(["BEGIN AUTH REQUEST", root, user, passw,
                                    "END AUTH REQUEST", ""]))
                if sck.recv(128) != "I LOVE YOU\n":
                    raise util.Abort("CVS pserver authentication failed")

                self.writep = self.readp = sck.makefile('r+')

        if not conntype and root.startswith(":local:"):
            conntype = "local"
            root = root[7:]

        if not conntype:
            # :ext:user@host/home/user/path/to/cvsroot
            if root.startswith(":ext:"):
                root = root[5:]
            m = re.match(r'(?:([^@:/]+)@)?([^:/]+):?(.*)', root)
            # Do not take Windows path "c:\foo\bar" for a connection strings
            if os.path.isdir(root) or not m:
                conntype = "local"
            else:
                conntype = "rsh"
                user, host, root = m.group(1), m.group(2), m.group(3)

        if conntype != "pserver":
            if conntype == "rsh":
                rsh = os.environ.get("CVS_RSH") or "ssh"
                if user:
                    cmd = [rsh, '-l', user, host] + cmd
                else:
                    cmd = [rsh, host] + cmd

            # popen2 does not support argument lists under Windows
            cmd = [util.shellquote(arg) for arg in cmd]
            cmd = util.quotecommand(' '.join(cmd))
            self.writep, self.readp = os.popen2(cmd, 'b')

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

        def chunkedread(fp, count):
            # file-objects returned by socked.makefile() do not handle
            # large read() requests very well.
            chunksize = 65536
            output = StringIO()
            while count > 0:
                data = fp.read(min(count, chunksize))
                if not data:
                    raise util.Abort("%d bytes missing from remote file" % count)
                count -= len(data)
                output.write(data)
            return output.getvalue()

        if rev.endswith("(DEAD)"):
            raise IOError

        args = ("-N -P -kk -r %s --" % rev).split()
        args.append(self.cvsrepo + '/' + name)
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
                data = chunkedread(self.readp, count)
            elif line.startswith(" "):
                data += line[1:]
            elif line.startswith("M "):
                pass
            elif line.startswith("Mbinary "):
                count = int(self.readp.readline()[:-1])
                data = chunkedread(self.readp, count)
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
        return (cl, {})

    def getcommit(self, rev):
        return self.changeset[rev]

    def gettags(self):
        return self.tags

    def getchangedfiles(self, rev, i):
        files = self.files[rev].keys()
        files.sort()
        return files

