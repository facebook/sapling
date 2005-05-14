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

def httphdr(type = "text/html"):
    print 'Content-type: %s\n' % type

def htmldoctype():
    print '<!DOCTYPE HTML PUBLIC "-//W3C//DTD HTML 3.2//EN">'

def htmlhead(title):
    print '<HTML>'
    print '<!-- created by hgweb 0.1 - jake@edge2.net -->'
    print '<HEAD><TITLE>%s</TITLE></HEAD>' % (title, )
    print '<style type="text/css">'
    print 'body { font-family: sans-serif; font-size: 12px; }'
    print 'table { font-size: 12px; }'
    print '.errmsg { font-size: 200%; color: red; }'
    print '.filename { font-size: 150%; color: purple; }'
    print '.plusline { color: green; }'
    print '.minusline { color: red; }'
    print '.atline { color: purple; }'
    print '</style>'

def startpage(title):
    httphdr()
    htmldoctype()
    htmlhead(title)
    print '<BODY>'

def endpage():
    print '</BODY>'
    print '</HTML>'



def ent_change(repo, nodeid, changes):
    hn = hg.hex(nodeid)
    i = repo.changelog.rev(nodeid)
    (h1, h2) = [ hg.hex(x) for x in repo.changelog.parents(nodeid) ]
    datestr = time.asctime(time.gmtime(float(changes[2].split(' ')[0])))
    print '<table width="100%" border="1">'
    print '\t<tr><td valign="top" width="10%%">author:</td>' + \
            '<td valign="top" width="20%%">%s</td>' % (obfuscate(changes[1]), )
    print '\t\t<td valign="top" width="10%%">description:</td>' + \
            '<td width="60%%">' + \
            '<a href="?cmd=chkin;nd=%s">%s</a></td></tr>' % \
            (hn, nl2br(changes[4]), )
    print '\t<tr><td>date:</td><td>%s UTC</td>' % (datestr, )
    print '\t\t<td valign="top">files:</td><td valign="top">'
    for f in changes[3]:
        print '\t\t%s&nbsp;&nbsp;' % f
    print '\t</td></tr>'
    print '\t<tr><td>revision:</td><td colspan="3">%d:<a ' % (i, ) + \
            'href="?cmd=chkin;nd=%s">%s</a></td></tr>' % (hn, hn, )
    print '</table><br />'

def ent_diff(a, b, fn):
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

def ent_checkin(repo, nodeid):
    startpage("Mercurial Web")

    changes = repo.changelog.read(nodeid)
    hn = hg.hex(nodeid)
    i = repo.changelog.rev(nodeid)
    parents = repo.changelog.parents(nodeid)
    (h1, h2) = [ hg.hex(x) for x in parents ]
    (i1, i2) = [ repo.changelog.rev(x) for x in parents ]
    datestr = time.asctime(time.gmtime(float(changes[2].split(' ')[0])))
    mf = repo.manifest.read(changes[0])
    print '<table width="100%" border="1">'
    print '\t<tr><td>revision:</td><td colspan="3">%d:' % (i, ),
    print '<a href="?cmd=chkin;nd=%s">%s</a></td></tr>' % (hn, hn, )
    print '\t<tr><td>parent(s):</td><td colspan="3">%d:' % (i1, )
    print '<a href="?cmd=chkin;nd=%s">%s</a>' % (h1, h1, ),
    if i2 != -1:
        print '&nbsp;&nbsp;%d:<a href="?cmd=chkin;nd=%s">%s</a>' % \
                (i2, h2, h2, ),
    else:
        print '&nbsp;&nbsp;%d:%s' % (i2, h2, ),
    print '</td></tr>'
    print '\t<tr><td>manifest:</td><td colspan="3">%d:' % \
            (repo.manifest.rev(changes[0]), ),
    print '<a href="?cmd=mf;nd=%s">%s</a></td></tr>' % \
            (hg.hex(changes[0]), hg.hex(changes[0]), )
    print '\t<tr><td valign="top" width="10%%">author:</td>' + \
            '<td valign="top" width="20%%">%s</td>' % (obfuscate(changes[1]), )
    print '\t\t<td valign="top" width="10%%">description:</td>' + \
            '<td width="60%%">' + \
            '<a href="?cmd=chkin;nd=%s">%s</a></td></tr>' % \
            (hn, nl2br(changes[4]), )
    print '\t<tr><td>date:</td><td>%s UTC</td>' % (datestr, )
    print '\t\t<td valign="top">files:</td><td valign="top">'
    for f in changes[3]:
        print '\t\t<a href="?cmd=file;nd=%s&fn=%s">%s</a>' % \
                (hg.hex(mf[f]), f, f, ),
        print '&nbsp;&nbsp;'
    print '\t</td></tr>'
    print '</table><br />'

    (c, a, d) = repo.diffrevs(parents[0], nodeid)
    change = repo.changelog.read(parents[0])
    mf2 = repo.manifest.read(change[0])
    for f in c:
        ent_diff(repo.file(f).read(mf2[f]), repo.file(f).read(mf[f]), f)
    for f in a:
        ent_diff('', repo.file(f).read(mf[f]), f)
    for f in d:
        ent_diff(repo.file(f).read(mf2[f]), '', f)

    endpage()


def ent_file(repo, nodeid, fn):
    print '<div class="filename">%s (%s)</div>' % (fn, hg.hex(nodeid), )
    print '<pre>'
    print cgi.escape(repo.file(fn).read(nodeid))
    print '</pre>'

def change_page():
    startpage("Mercurial Web")
    print '<table width="100%" align="center">'
    cl = []
    for i in xrange(repo.changelog.count()):
        n = repo.changelog.node(i)
        cl.append((n, repo.changelog.read(n)))
    cl.reverse()
    for n, ch in cl:
        print '<tr><td>'
        ent_change(repo, n, ch)
        print '</td></th>'

    print '</table>'
    endpage()

args = cgi.parse()

ui = hg.ui()
repo = hg.repository(ui, repo_path)

if not args.has_key('cmd'):
    change_page()
    
elif args['cmd'][0] == 'chkin':
    if not args.has_key('nd'):
        print '<div class="errmsg">No Node!</div>'
    else:
        ent_checkin(repo, hg.bin(args['nd'][0]))

elif args['cmd'][0] == 'file':
    startpage("Mercurial Web")

    if not args.has_key('nd'):
        print '<div class="errmsg">No Node!</div>'
    elif not args.has_key('fn'):
        print '<div class="errmsg">No Filename!</div>'
    else:
        ent_file(repo, hg.bin(args['nd'][0]), args['fn'][0])
    endpage()

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
    startpage("Mercurial Web Error")
    print '<div class="errmsg">unknown command: ', args['cmd'][0], '</div>'
    endpage()
