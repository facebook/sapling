#!/usr/bin/env python
#
# hgweb.py - 0.2 - 21 May 2005 - (c) 2005 Jake Edge <jake@edge2.net>
#    - web interface to a mercurial repository
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

# useful for debugging
import cgitb
cgitb.enable()

import os, cgi, time, re, difflib, sys, zlib
from mercurial import hg, mdiff

def nl2br(text):
    return re.sub('\n', '<br />', text)

def obfuscate(text):
    l = []
    for c in text:
        l.append('&#%d;' % ord(c))
    return ''.join(l)

def httphdr(type):
    print 'Content-type: %s\n' % type

class template:
    def __init__(self, tmpl_dir):
        self.tmpl_dir = tmpl_dir
    def do_page(self, tmpl_fn, **map):
        out = []
        txt = file(os.path.join(self.tmpl_dir, tmpl_fn)).read()
        while txt:
            m = re.search(r"#([a-zA-Z0-9]+)#", txt)
            if m:
                out.append(txt[:m.start(0)])
                v = map.get(m.group(1), "")
                if callable(v):
                   for y in v(**map): out.append(y)
                else:
                   out.append(str(v))
                txt = txt[m.end(0):]
            else:
                out.append(txt)
                txt = ''
        return ''.join(out)

class page:
    def __init__(self, tmpl_dir = "", type="text/html", title="Mercurial Web", 
            charset="ISO-8859-1"):
        self.tmpl = template(tmpl_dir)

        print 'Content-type: %s; charset=%s\n' % (type, charset)
        print self.tmpl.do_page('htmlstart.tmpl', title = title)

    def endpage(self):
        print '</BODY>'
        print '</HTML>'

    def show_diff(self, a, b, fn):
        a = a.splitlines(1)
        b = b.splitlines(1)
        l = difflib.unified_diff(a, b, fn, fn)
        print '<pre>'
        for line in l:
            line = cgi.escape(line[:-1])
            if line.startswith('+'):
                print '<span class="plusline">%s</span>' % (line, )
            elif line.startswith('-'):
                print '<span class="minusline">%s</span>' % (line, )
            elif line.startswith('@'):
                print '<span class="atline">%s</span>' % (line, )
            else:
                print line
        print '</pre>'

class errpage(page):
    def __init__(self):
        page.__init__(self, title="Mercurial Web Error Page")

class change_list(page):
    def __init__(self, repo, tmpl_dir, reponame, numchanges = 50):
        page.__init__(self, tmpl_dir)
        self.repo = repo
        self.numchanges = numchanges
        print self.tmpl.do_page('changestitle.tmpl', reponame=reponame)

    def content(self, hi=None):
        cl = []
        count = self.repo.changelog.count()
        if not hi:
            hi = count
        elif hi < self.numchanges:
            hi = self.numchanges

        start = 0
        if hi - self.numchanges >= 0:
            start = hi - self.numchanges

        nav = "Displaying Revisions: %d-%d" % (start, hi-1)
        if start != 0:
            nav = ('<a href="?cmd=changes;hi=%d">Previous %d</a>&nbsp;&nbsp;' \
                    % (start, self.numchanges)) + nav
        if hi != count:
            if hi + self.numchanges <= count:
                nav += '&nbsp;&nbsp;<a href="?cmd=changes;hi=%d">Next %d</a>' \
                        % (hi + self.numchanges, self.numchanges)
            else:
                nav += '&nbsp;&nbsp;<a href="?cmd=changes">Next %d</a>' % \
                        self.numchanges

        print '<center>%s</center>' % nav

        for i in xrange(start, hi):
            n = self.repo.changelog.node(i)
            cl.append((n, self.repo.changelog.read(n)))
        cl.reverse()

        print '<table summary="" width="100%" align="center">'
        for n, ch in cl:
            print '<tr><td>'
            self.change_table(n, ch)
            print '</td></tr>'
        print '</table>'

        print '<center>%s</center>' % nav

    def change_table(self, nodeid, changes):
        hn = hg.hex(nodeid)
        i = self.repo.changelog.rev(nodeid)
        (h1, h2) = [ hg.hex(x) for x in self.repo.changelog.parents(nodeid) ]
        datestr = time.asctime(time.gmtime(float(changes[2].split(' ')[0])))
        files = []
        for f in changes[3]:
            files.append('<a href="?cmd=file;cs=%s;fn=%s">%s</a>&nbsp;&nbsp;' \
                % (hn, f, cgi.escape(f)))
        print self.tmpl.do_page('change_table.tmpl', 
                author=obfuscate(changes[1]),
                desc=nl2br(cgi.escape(changes[4])), date=datestr, 
                files=' '.join(files), revnum=i, revnode=hn)

class checkin(page):
    def __init__(self, repo, tmpl_dir, nodestr):
        page.__init__(self, tmpl_dir)
        self.repo = repo
        self.node = hg.bin(nodestr)
        self.nodestr = nodestr
        print '<h3>Checkin: %s</h3>' % nodestr

    def content(self):
        changes = self.repo.changelog.read(self.node)
        i = self.repo.changelog.rev(self.node)
        parents = self.repo.changelog.parents(self.node)
        (h1, h2) = [ hg.hex(x) for x in parents ]
        (i1, i2) = [ self.repo.changelog.rev(x) for x in parents ]
        datestr = time.asctime(time.gmtime(float(changes[2].split(' ')[0])))
        mf = self.repo.manifest.read(changes[0])
        files = []
        for f in changes[3]:
            files.append('<a href="?cmd=file;nd=%s;fn=%s">%s</a>&nbsp;&nbsp;' \
                % (hg.hex(mf[f]), f, cgi.escape(f)))
        p2link = h2
        if i2 != -1:
            p2link = '<a href="?cmd=chkin;nd=%s">%s</a>' % (h2, h2)

        print self.tmpl.do_page('checkin.tmpl', revnum=i, revnode=self.nodestr,
                p1num=i1, p1node=h1, p2num=i2, p2node=h2, p2link=p2link,
                mfnum=self.repo.manifest.rev(changes[0]), 
                mfnode=hg.hex(changes[0]), author=obfuscate(changes[1]),
                desc=nl2br(cgi.escape(changes[4])), date=datestr,
                files=' '.join(files))

        (c, a, d) = self.repo.diffrevs(parents[0], self.node)
        change = self.repo.changelog.read(parents[0])
        mf2 = self.repo.manifest.read(change[0])
        for f in c:
            self.show_diff(self.repo.file(f).read(mf2[f]), \
                    self.repo.file(f).read(mf[f]), f)
        for f in a:
            self.show_diff('', self.repo.file(f).read(mf[f]), f)
        for f in d:
            self.show_diff(self.repo.file(f).read(mf2[f]), '', f)

class filepage(page):
    def __init__(self, repo, tmpl_dir, fn, node=None, cs=None):
        page.__init__(self, tmpl_dir)
        self.repo = repo
        self.fn = fn
        if cs: 
            chng = self.repo.changelog.read(hg.bin(cs))
            mf = self.repo.manifest.read(chng[0])
            self.node = mf[self.fn]
            self.nodestr = hg.hex(self.node)
        else:
            self.nodestr = node
            self.node = hg.bin(node)
        print '<div class="filename">%s (%s)</div>' % \
                (cgi.escape(self.fn), self.nodestr, )
        print '<a href="?cmd=hist;fn=%s">history</a><br />' % self.fn

    def content(self):
        print '<pre>'
        print cgi.escape(self.repo.file(self.fn).read(self.node))
        print '</pre>'

class mfpage(page):
    def __init__(self, repo, tmpl_dir, node):
        page.__init__(self, tmpl_dir)
        self.repo = repo
        self.nodestr = node
        self.node = hg.bin(node)

    def content(self):
        mf = self.repo.manifest.read(self.node)
        fns = mf.keys()
        fns.sort()
        print self.tmpl.do_page('mftitle.tmpl', node = self.nodestr)
        for f in fns:
            print self.tmpl.do_page('mfentry.tmpl', fn=f, node=hg.hex(mf[f]))

class histpage(page):
    def __init__(self, repo, tmpl_dir, fn):
        page.__init__(self, tmpl_dir)
        self.repo = repo
        self.fn = fn

    def content(self):
        print '<div class="filehist">File History: %s</div>' % self.fn
        r = self.repo.file(self.fn)
        print '<br />'
        print '<table summary="" width="100%" align="center">'
        for i in xrange(r.count()-1, -1, -1):
            print '<tr><td>'
            self.hist_ent(i, r)
            print '</tr></td>'
        print '</table>'

    def hist_ent(self, i, r):
        n = r.node(i)
        (p1, p2) = r.parents(n)
        (h, h1, h2) = map(hg.hex, (n, p1, p2))
        (i1, i2) = map(r.rev, (p1, p2))
        ci = r.linkrev(n)
        cn = self.repo.changelog.node(ci)
        cs = hg.hex(cn)
        changes = self.repo.changelog.read(cn)
        datestr = time.asctime(time.gmtime(float(changes[2].split(' ')[0])))
        p2entry = ''
        if i2 != -1:
            p2entry = '&nbsp;&nbsp;%d:<a href="?cmd=file;nd=%s;fn=%s">%s</a>' \
                    % (i2, h2, self.fn, h2 ),
        print self.tmpl.do_page('hist_ent.tmpl', author=obfuscate(changes[1]),
                csnode=cs, desc=nl2br(cgi.escape(changes[4])), 
                date = datestr, fn=self.fn, revnode=h, p1num = i1,
                p1node=h1, p2entry=p2entry)
                
class hgweb:
    repo_path = "."
    numchanges = 50
    tmpl_dir = "templates"

    def __init__(self):
        pass

    def run(self):

        args = cgi.parse()

        ui = hg.ui()
        repo = hg.repository(ui, self.repo_path)

        if not args.has_key('cmd') or args['cmd'][0] == 'changes':
            page = change_list(repo, self.tmpl_dir, 'Mercurial', 
                    self.numchanges)
            hi = args.get('hi', ( repo.changelog.count(), ))
            page.content(hi = int(hi[0]))
            page.endpage()
            
        elif args['cmd'][0] == 'chkin':
            if not args.has_key('nd'):
                page = errpage()
                print '<div class="errmsg">No Node!</div>'
            else:
                page = checkin(repo, self.tmpl_dir, args['nd'][0])
                page.content()
            page.endpage()

        elif args['cmd'][0] == 'file':
            if not (args.has_key('nd') and args.has_key('fn')) and \
                    not (args.has_key('cs') and args.has_key('fn')):
                page = errpage()
                print '<div class="errmsg">Invalid Args!</div>'
            else:
                if args.has_key('nd'):
                    page = filepage(repo, self.tmpl_dir, 
                            args['fn'][0], node=args['nd'][0])
                else:
                    page = filepage(repo, self.tmpl_dir,
                            args['fn'][0], cs=args['cs'][0])
                page.content()
            page.endpage()

        elif args['cmd'][0] == 'mf':
            if not args.has_key('nd'):
                page = errpage()
                print '<div class="errmsg">No Node!</div>'
            else:
                page = mfpage(repo, self.tmpl_dir, args['nd'][0])
                page.content()
            page.endpage()

        elif args['cmd'][0] == 'hist':
            if not args.has_key('fn'):
                page = errpage()
                print '<div class="errmsg">No Filename!</div>'
            else:
                page = histpage(repo, self.tmpl_dir, args['fn'][0])
                page.content()
            page.endpage()

        elif args['cmd'][0] == 'branches':
            httphdr("text/plain")
            nodes = []
            if args.has_key('nodes'):
                nodes = map(hg.bin, args['nodes'][0].split(" "))
            for b in repo.branches(nodes):
                print " ".join(map(hg.hex, b))

        elif args['cmd'][0] == 'between':
            httphdr("text/plain")
            nodes = []
            if args.has_key('pairs'):
                pairs = [ map(hg.bin, p.split("-"))
                          for p in args['pairs'][0].split(" ") ]
            for b in repo.between(pairs):
                print " ".join(map(hg.hex, b))

        elif args['cmd'][0] == 'changegroup':
            httphdr("application/hg-changegroup")
            nodes = []
            if args.has_key('roots'):
                nodes = map(hg.bin, args['roots'][0].split(" "))

            z = zlib.compressobj()
            for chunk in repo.changegroup(nodes):
                sys.stdout.write(z.compress(chunk))

            sys.stdout.write(z.flush())

        else:
            page = errpage()
            print '<div class="errmsg">unknown command: %s</div>' % \
                    cgi.escape(args['cmd'][0])
            page.endpage()

if __name__ == "__main__":
    hgweb().run()
