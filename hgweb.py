#!/usr/bin/env python
#
# hgweb.py - 0.1 - 9 May 2005 - (c) 2005 Jake Edge <jake@edge2.net>
#    - web interface to a mercurial repository
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

# useful for debugging
import cgitb
cgitb.enable()

import os, cgi, time, re, difflib, sys, zlib
from mercurial import hg, mdiff

repo_path = "."  # change as needed

def nl2br(text):
    return re.sub('\n', '<br />', text)

def obfuscate(text):
    l = []
    for c in text:
        l.append('&#%d;' % ord(c))
    return ''.join(l)

def httphdr(type):
    print 'Content-type: %s\n' % type

class page:
    def __init__(self, type="text/html", title="Mercurial Web", 
            charset="ISO-8859-1"):
        print 'Content-type: %s; charset=%s\n' % (type, charset)
        print '<!DOCTYPE HTML PUBLIC "-//W3C//DTD HTML 4.01 Transitional//EN">'
        print '<HTML>'
        print '<!-- created by hgweb 0.1 - jake@edge2.net -->'
        print '<HEAD><TITLE>%s</TITLE>' % title
        print '<style type="text/css">'
        print 'body { font-family: sans-serif; font-size: 12px; }'
        print 'table { font-size: 12px; }'
        print '.errmsg { font-size: 200%; color: red; }'
        print '.filename { font-size: 150%; color: purple; }'
        print '.plusline { color: green; }'
        print '.minusline { color: red; }'
        print '.atline { color: purple; }'
        print '</style>'
        print '</HEAD>'
        print '<BODY>'

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
    def __init__(self, repo, reponame):
        page.__init__(self)
        self.repo = repo
        print '<h3>Changes For: %s</h3>' % reponame

    def content(self, start='tip', end='0', boundtype='rev'):
        print '<table summary="" width="100%" align="center">'
        cl = []
        for i in xrange(self.repo.changelog.count()):
            n = self.repo.changelog.node(i)
            cl.append((n, self.repo.changelog.read(n)))
        cl.reverse()
        for n, ch in cl:
            print '<tr><td>'
            self.change_table(n, ch)
            print '</td></tr>'

        print '</table>'

    def change_table(self, nodeid, changes):
        hn = hg.hex(nodeid)
        i = self.repo.changelog.rev(nodeid)
        (h1, h2) = [ hg.hex(x) for x in self.repo.changelog.parents(nodeid) ]
        datestr = time.asctime(time.gmtime(float(changes[2].split(' ')[0])))
        print '<table summary="" width="100%" border="1">'
        print '\t<tr><td valign="top" width="10%">author:</td>' + \
                '<td valign="top" width="20%%">%s</td>' % \
                (obfuscate(changes[1]), )
        print '\t\t<td valign="top" width="10%">description:</td>' + \
                '<td width="60%">' + \
                '<a href="?cmd=chkin;nd=%s">%s</a></td></tr>' % \
                (hn, nl2br(cgi.escape(changes[4])), )
        print '\t<tr><td>date:</td><td>%s UTC</td>' % (datestr, )
        print '\t\t<td valign="top">files:</td><td valign="top">'
        for f in changes[3]:
            print '\t\t<a href="?cmd=file;cs=%s;fn=%s">%s</a>&nbsp;&nbsp;' % \
                    (hn, f, cgi.escape(f), )
        print '\t</td></tr>'
        print '\t<tr><td>revision:</td><td colspan="3">%d:<a ' % (i, ) + \
                'href="?cmd=chkin;nd=%s">%s</a></td></tr>' % (hn, hn, )
        print '</table><br />'

class checkin(page):
    def __init__(self, repo, nodestr):
        page.__init__(self)
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
        print '<table summary="" width="100%" border="1">'
        print '\t<tr><td>revision:</td><td colspan="3">%d:' % (i, ),
        print '<a href="?cmd=chkin;nd=%s">%s</a></td></tr>' % \
                (self.nodestr, self.nodestr, )
        print '\t<tr><td>parent(s):</td><td colspan="3">%d:' % (i1, )
        print '<a href="?cmd=chkin;nd=%s">%s</a>' % (h1, h1, ),
        if i2 != -1:
            print '&nbsp;&nbsp;%d:<a href="?cmd=chkin;nd=%s">%s</a>' % \
                    (i2, h2, h2, ),
        else:
            print '&nbsp;&nbsp;%d:%s' % (i2, h2, ),
        print '</td></tr>'
        print '\t<tr><td>manifest:</td><td colspan="3">%d:' % \
                (self.repo.manifest.rev(changes[0]), ),
        print '<a href="?cmd=mf;nd=%s">%s</a></td></tr>' % \
                (hg.hex(changes[0]), hg.hex(changes[0]), )
        print '\t<tr><td valign="top" width="10%">author:</td>' + \
                '<td valign="top" width="20%%">%s</td>' % \
                (obfuscate(changes[1]), )
        print '\t\t<td valign="top" width="10%">description:</td>' + \
                '<td width="60%">' + \
                '<a href="?cmd=chkin;nd=%s">%s</a></td></tr>' % \
                (self.nodestr, nl2br(cgi.escape(changes[4])), )
        print '\t<tr><td>date:</td><td>%s UTC</td>' % (datestr, )
        print '\t\t<td valign="top">files:</td><td valign="top">'
        for f in changes[3]:
            print '\t\t<a href="?cmd=file;nd=%s&fn=%s">%s</a>' % \
                    (hg.hex(mf[f]), f, cgi.escape(f), ),
            print '&nbsp;&nbsp;'
        print '\t</td></tr>'
        print '</table><br />'

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
    def __init__(self, repo, fn, node=None, cs=None):
        page.__init__(self)
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

args = cgi.parse()

ui = hg.ui()
repo = hg.repository(ui, repo_path)

if not args.has_key('cmd'):
    page = change_list(repo, 'Mercurial')
    page.content()
    page.endpage()
    
elif args['cmd'][0] == 'chkin':
    if not args.has_key('nd'):
        page = errpage()
        print '<div class="errmsg">No Node!</div>'
    else:
        page = checkin(repo, args['nd'][0])
        page.content()
    page.endpage()

elif args['cmd'][0] == 'file':
    if not (args.has_key('nd') and args.has_key('fn')) and \
            not (args.has_key('cs') and args.has_key('fn')):
        page = errpage()
        print '<div class="errmsg">Invalid Args!</div>'
    else:
        if args.has_key('nd'):
            page = filepage(repo, args['fn'][0], node=args['nd'][0])
        else:
            page = filepage(repo, args['fn'][0], cs=args['cs'][0])
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
