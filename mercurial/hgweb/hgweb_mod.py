# hgweb/hgweb_mod.py - Web interface for a repository.
#
# Copyright 21 May 2005 - (c) 2005 Jake Edge <jake@edge2.net>
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import os, mimetypes, re, mimetools, cStringIO
from mercurial.node import hex, nullid, short
from mercurial.repo import RepoError
from mercurial import mdiff, ui, hg, util, archival, patch, hook
from mercurial import revlog, templater, templatefilters, changegroup
from common import get_mtime, style_map, paritygen, countgen, get_contact
from common import ErrorResponse
from common import HTTP_OK, HTTP_BAD_REQUEST, HTTP_NOT_FOUND, HTTP_SERVER_ERROR
from request import wsgirequest
import webcommands, protocol

shortcuts = {
    'cl': [('cmd', ['changelog']), ('rev', None)],
    'sl': [('cmd', ['shortlog']), ('rev', None)],
    'cs': [('cmd', ['changeset']), ('node', None)],
    'f': [('cmd', ['file']), ('filenode', None)],
    'fl': [('cmd', ['filelog']), ('filenode', None)],
    'fd': [('cmd', ['filediff']), ('node', None)],
    'fa': [('cmd', ['annotate']), ('filenode', None)],
    'mf': [('cmd', ['manifest']), ('manifest', None)],
    'ca': [('cmd', ['archive']), ('node', None)],
    'tags': [('cmd', ['tags'])],
    'tip': [('cmd', ['changeset']), ('node', ['tip'])],
    'static': [('cmd', ['static']), ('file', None)]
}

def _up(p):
    if p[0] != "/":
        p = "/" + p
    if p[-1] == "/":
        p = p[:-1]
    up = os.path.dirname(p)
    if up == "/":
        return "/"
    return up + "/"

def revnavgen(pos, pagelen, limit, nodefunc):
    def seq(factor, limit=None):
        if limit:
            yield limit
            if limit >= 20 and limit <= 40:
                yield 50
        else:
            yield 1 * factor
            yield 3 * factor
        for f in seq(factor * 10):
            yield f

    def nav(**map):
        l = []
        last = 0
        for f in seq(1, pagelen):
            if f < pagelen or f <= last:
                continue
            if f > limit:
                break
            last = f
            if pos + f < limit:
                l.append(("+%d" % f, hex(nodefunc(pos + f).node())))
            if pos - f >= 0:
                l.insert(0, ("-%d" % f, hex(nodefunc(pos - f).node())))

        try:
            yield {"label": "(0)", "node": hex(nodefunc('0').node())}

            for label, node in l:
                yield {"label": label, "node": node}

            yield {"label": "tip", "node": "tip"}
        except RepoError:
            pass

    return nav

class hgweb(object):
    def __init__(self, repo, name=None):
        if isinstance(repo, str):
            parentui = ui.ui(report_untrusted=False, interactive=False)
            self.repo = hg.repository(parentui, repo)
        else:
            self.repo = repo

        hook.redirect(True)
        self.mtime = -1
        self.reponame = name
        self.archives = 'zip', 'gz', 'bz2'
        self.stripecount = 1
        self._capabilities = None
        # a repo owner may set web.templates in .hg/hgrc to get any file
        # readable by the user running the CGI script
        self.templatepath = self.config("web", "templates",
                                        templater.templatepath(),
                                        untrusted=False)

    # The CGI scripts are often run by a user different from the repo owner.
    # Trust the settings from the .hg/hgrc files by default.
    def config(self, section, name, default=None, untrusted=True):
        return self.repo.ui.config(section, name, default,
                                   untrusted=untrusted)

    def configbool(self, section, name, default=False, untrusted=True):
        return self.repo.ui.configbool(section, name, default,
                                       untrusted=untrusted)

    def configlist(self, section, name, default=None, untrusted=True):
        return self.repo.ui.configlist(section, name, default,
                                       untrusted=untrusted)

    def refresh(self):
        mtime = get_mtime(self.repo.root)
        if mtime != self.mtime:
            self.mtime = mtime
            self.repo = hg.repository(self.repo.ui, self.repo.root)
            self.maxchanges = int(self.config("web", "maxchanges", 10))
            self.stripecount = int(self.config("web", "stripes", 1))
            self.maxshortchanges = int(self.config("web", "maxshortchanges", 60))
            self.maxfiles = int(self.config("web", "maxfiles", 10))
            self.allowpull = self.configbool("web", "allowpull", True)
            self.encoding = self.config("web", "encoding", util._encoding)
            self._capabilities = None

    def capabilities(self):
        if self._capabilities is not None:
            return self._capabilities
        caps = ['lookup', 'changegroupsubset']
        if self.configbool('server', 'uncompressed'):
            caps.append('stream=%d' % self.repo.changelog.version)
        if changegroup.bundlepriority:
            caps.append('unbundle=%s' % ','.join(changegroup.bundlepriority))
        self._capabilities = caps
        return caps

    def run(self):
        if not os.environ.get('GATEWAY_INTERFACE', '').startswith("CGI/1."):
            raise RuntimeError("This function is only intended to be called while running as a CGI script.")
        import mercurial.hgweb.wsgicgi as wsgicgi
        wsgicgi.launch(self)

    def __call__(self, env, respond):
        req = wsgirequest(env, respond)
        self.run_wsgi(req)
        return req

    def run_wsgi(self, req):

        self.refresh()

        # expand form shortcuts

        for k in shortcuts.iterkeys():
            if k in req.form:
                for name, value in shortcuts[k]:
                    if value is None:
                        value = req.form[k]
                    req.form[name] = value
                del req.form[k]

        # work with CGI variables to create coherent structure
        # use SCRIPT_NAME, PATH_INFO and QUERY_STRING as well as our REPO_NAME

        req.url = req.env['SCRIPT_NAME']
        if not req.url.endswith('/'):
            req.url += '/'
        if 'REPO_NAME' in req.env:
            req.url += req.env['REPO_NAME'] + '/'

        if 'PATH_INFO' in req.env:
            parts = req.env['PATH_INFO'].strip('/').split('/')
            repo_parts = req.env.get('REPO_NAME', '').split('/')
            if parts[:len(repo_parts)] == repo_parts:
                parts = parts[len(repo_parts):]
            query = '/'.join(parts)
        else:
            query = req.env['QUERY_STRING'].split('&', 1)[0]
            query = query.split(';', 1)[0]

        # translate user-visible url structure to internal structure

        args = query.split('/', 2)
        if 'cmd' not in req.form and args and args[0]:

            cmd = args.pop(0)
            style = cmd.rfind('-')
            if style != -1:
                req.form['style'] = [cmd[:style]]
                cmd = cmd[style+1:]

            # avoid accepting e.g. style parameter as command
            if hasattr(webcommands, cmd) or hasattr(protocol, cmd):
                req.form['cmd'] = [cmd]

            if args and args[0]:
                node = args.pop(0)
                req.form['node'] = [node]
            if args:
                req.form['file'] = args

            if cmd == 'static':
                req.form['file'] = req.form['node']
            elif cmd == 'archive':
                fn = req.form['node'][0]
                for type_, spec in self.archive_specs.iteritems():
                    ext = spec[2]
                    if fn.endswith(ext):
                        req.form['node'] = [fn[:-len(ext)]]
                        req.form['type'] = [type_]

        # process this if it's a protocol request

        cmd = req.form.get('cmd', [''])[0]
        if cmd in protocol.__all__:
            method = getattr(protocol, cmd)
            method(self, req)
            return

        # process the web interface request

        try:

            tmpl = self.templater(req)
            try:
                ctype = tmpl('mimetype', encoding=self.encoding)
                ctype = templater.stringify(ctype)
            except KeyError:
                # old templates with inline HTTP headers?
                if 'mimetype' in tmpl:
                    raise
                header = tmpl('header', encoding=self.encoding)
                header_file = cStringIO.StringIO(templater.stringify(header))
                msg = mimetools.Message(header_file, 0)
                ctype = msg['content-type']

            if cmd == '':
                req.form['cmd'] = [tmpl.cache['default']]
                cmd = req.form['cmd'][0]

            if cmd not in webcommands.__all__:
                msg = 'no such method: %s' % cmd
                raise ErrorResponse(HTTP_BAD_REQUEST, msg)
            elif cmd == 'file' and 'raw' in req.form.get('style', []):
                self.ctype = ctype
                content = webcommands.rawfile(self, req, tmpl)
            else:
                content = getattr(webcommands, cmd)(self, req, tmpl)
                req.respond(HTTP_OK, ctype)

            req.write(content)
            del tmpl

        except revlog.LookupError, err:
            req.respond(HTTP_NOT_FOUND, ctype)
            msg = str(err)
            if 'manifest' not in msg:
                msg = 'revision not found: %s' % err.name
            req.write(tmpl('error', error=msg))
        except (RepoError, revlog.RevlogError), inst:
            req.respond(HTTP_SERVER_ERROR, ctype)
            req.write(tmpl('error', error=str(inst)))
        except ErrorResponse, inst:
            req.respond(inst.code, ctype)
            req.write(tmpl('error', error=inst.message))

    def templater(self, req):

        # determine scheme, port and server name
        # this is needed to create absolute urls

        proto = req.env.get('wsgi.url_scheme')
        if proto == 'https':
            proto = 'https'
            default_port = "443"
        else:
            proto = 'http'
            default_port = "80"

        port = req.env["SERVER_PORT"]
        port = port != default_port and (":" + port) or ""
        urlbase = '%s://%s%s' % (proto, req.env['SERVER_NAME'], port)
        staticurl = self.config("web", "staticurl") or req.url + 'static/'
        if not staticurl.endswith('/'):
            staticurl += '/'

        # some functions for the templater

        def header(**map):
            header = tmpl('header', encoding=self.encoding, **map)
            if 'mimetype' not in tmpl:
                # old template with inline HTTP headers
                header_file = cStringIO.StringIO(templater.stringify(header))
                msg = mimetools.Message(header_file, 0)
                header = header_file.read()
            yield header

        def footer(**map):
            yield tmpl("footer", **map)

        def motd(**map):
            yield self.config("web", "motd", "")

        def sessionvars(**map):
            fields = []
            if 'style' in req.form:
                style = req.form['style'][0]
                if style != self.config('web', 'style', ''):
                    fields.append(('style', style))

            separator = req.url[-1] == '?' and ';' or '?'
            for name, value in fields:
                yield dict(name=name, value=value, separator=separator)
                separator = ';'

        # figure out which style to use

        style = self.config("web", "style", "")
        if 'style' in req.form:
            style = req.form['style'][0]
        mapfile = style_map(self.templatepath, style)

        if not self.reponame:
            self.reponame = (self.config("web", "name")
                             or req.env.get('REPO_NAME')
                             or req.url.strip('/') or self.repo.root)

        # create the templater

        tmpl = templater.templater(mapfile, templatefilters.filters,
                                   defaults={"url": req.url,
                                             "staticurl": staticurl,
                                             "urlbase": urlbase,
                                             "repo": self.reponame,
                                             "header": header,
                                             "footer": footer,
                                             "motd": motd,
                                             "sessionvars": sessionvars
                                            })
        return tmpl

    def archivelist(self, nodeid):
        allowed = self.configlist("web", "allow_archive")
        for i, spec in self.archive_specs.iteritems():
            if i in allowed or self.configbool("web", "allow" + i):
                yield {"type" : i, "extension" : spec[2], "node" : nodeid}

    def listfilediffs(self, tmpl, files, changeset):
        for f in files[:self.maxfiles]:
            yield tmpl("filedifflink", node=hex(changeset), file=f)
        if len(files) > self.maxfiles:
            yield tmpl("fileellipses")

    def siblings(self, siblings=[], hiderev=None, **args):
        siblings = [s for s in siblings if s.node() != nullid]
        if len(siblings) == 1 and siblings[0].rev() == hiderev:
            return
        for s in siblings:
            d = {'node': hex(s.node()), 'rev': s.rev()}
            if hasattr(s, 'path'):
                d['file'] = s.path()
            d.update(args)
            yield d

    def renamelink(self, fl, node):
        r = fl.renamed(node)
        if r:
            return [dict(file=r[0], node=hex(r[1]))]
        return []

    def nodetagsdict(self, node):
        return [{"name": i} for i in self.repo.nodetags(node)]

    def nodebranchdict(self, ctx):
        branches = []
        branch = ctx.branch()
        # If this is an empty repo, ctx.node() == nullid,
        # ctx.branch() == 'default', but branchtags() is
        # an empty dict. Using dict.get avoids a traceback.
        if self.repo.branchtags().get(branch) == ctx.node():
            branches.append({"name": branch})
        return branches

    def nodeinbranch(self, ctx):
        branches = []
        branch = ctx.branch()
        if branch != 'default' and self.repo.branchtags().get(branch) != ctx.node():
            branches.append({"name": branch})
        return branches

    def nodebranchnodefault(self, ctx):
        branches = []
        branch = ctx.branch()
        if branch != 'default':
            branches.append({"name": branch})
        return branches

    def showtag(self, tmpl, t1, node=nullid, **args):
        for t in self.repo.nodetags(node):
            yield tmpl(t1, tag=t, **args)

    def diff(self, tmpl, node1, node2, files):
        def filterfiles(filters, files):
            l = [x for x in files if x in filters]

            for t in filters:
                if t and t[-1] != os.sep:
                    t += os.sep
                l += [x for x in files if x.startswith(t)]
            return l

        parity = paritygen(self.stripecount)
        def diffblock(diff, f, fn):
            yield tmpl("diffblock",
                       lines=prettyprintlines(diff),
                       parity=parity.next(),
                       file=f,
                       filenode=hex(fn or nullid))

        blockcount = countgen()
        def prettyprintlines(diff):
            blockno = blockcount.next()
            for lineno, l in enumerate(diff.splitlines(1)):
                if blockno == 0:
                    lineno = lineno + 1
                else:
                    lineno = "%d.%d" % (blockno, lineno + 1)
                if l.startswith('+'):
                    ltype = "difflineplus"
                elif l.startswith('-'):
                    ltype = "difflineminus"
                elif l.startswith('@'):
                    ltype = "difflineat"
                else:
                    ltype = "diffline"
                yield tmpl(ltype,
                           line=l,
                           lineid="l%s" % lineno,
                           linenumber="% 8s" % lineno)

        r = self.repo
        c1 = r.changectx(node1)
        c2 = r.changectx(node2)
        date1 = util.datestr(c1.date())
        date2 = util.datestr(c2.date())

        modified, added, removed, deleted, unknown = r.status(node1, node2)[:5]
        if files:
            modified, added, removed = map(lambda x: filterfiles(files, x),
                                           (modified, added, removed))

        diffopts = patch.diffopts(self.repo.ui, untrusted=True)
        for f in modified:
            to = c1.filectx(f).data()
            tn = c2.filectx(f).data()
            yield diffblock(mdiff.unidiff(to, date1, tn, date2, f, f,
                                          opts=diffopts), f, tn)
        for f in added:
            to = None
            tn = c2.filectx(f).data()
            yield diffblock(mdiff.unidiff(to, date1, tn, date2, f, f,
                                          opts=diffopts), f, tn)
        for f in removed:
            to = c1.filectx(f).data()
            tn = None
            yield diffblock(mdiff.unidiff(to, date1, tn, date2, f, f,
                                          opts=diffopts), f, tn)

    def changelog(self, tmpl, ctx, shortlog=False):
        def changelist(limit=0,**map):
            cl = self.repo.changelog
            l = [] # build a list in forward order for efficiency
            for i in xrange(start, end):
                ctx = self.repo.changectx(i)
                n = ctx.node()
                showtags = self.showtag(tmpl, 'changelogtag', n)

                l.insert(0, {"parity": parity.next(),
                             "author": ctx.user(),
                             "parent": self.siblings(ctx.parents(), i - 1),
                             "child": self.siblings(ctx.children(), i + 1),
                             "changelogtag": showtags,
                             "desc": ctx.description(),
                             "date": ctx.date(),
                             "files": self.listfilediffs(tmpl, ctx.files(), n),
                             "rev": i,
                             "node": hex(n),
                             "tags": self.nodetagsdict(n),
                             "inbranch": self.nodeinbranch(ctx),
                             "branches": self.nodebranchdict(ctx)})

            if limit > 0:
                l = l[:limit]

            for e in l:
                yield e

        maxchanges = shortlog and self.maxshortchanges or self.maxchanges
        cl = self.repo.changelog
        count = cl.count()
        pos = ctx.rev()
        start = max(0, pos - maxchanges + 1)
        end = min(count, start + maxchanges)
        pos = end - 1
        parity = paritygen(self.stripecount, offset=start-end)

        changenav = revnavgen(pos, maxchanges, count, self.repo.changectx)

        return tmpl(shortlog and 'shortlog' or 'changelog',
                    changenav=changenav,
                    node=hex(cl.tip()),
                    rev=pos, changesets=count,
                    entries=lambda **x: changelist(limit=0,**x),
                    latestentry=lambda **x: changelist(limit=1,**x),
                    archives=self.archivelist("tip"))

    def search(self, tmpl, query):

        def changelist(**map):
            cl = self.repo.changelog
            count = 0
            qw = query.lower().split()

            def revgen():
                for i in xrange(cl.count() - 1, 0, -100):
                    l = []
                    for j in xrange(max(0, i - 100), i + 1):
                        ctx = self.repo.changectx(j)
                        l.append(ctx)
                    l.reverse()
                    for e in l:
                        yield e

            for ctx in revgen():
                miss = 0
                for q in qw:
                    if not (q in ctx.user().lower() or
                            q in ctx.description().lower() or
                            q in " ".join(ctx.files()).lower()):
                        miss = 1
                        break
                if miss:
                    continue

                count += 1
                n = ctx.node()
                showtags = self.showtag(tmpl, 'changelogtag', n)

                yield tmpl('searchentry',
                           parity=parity.next(),
                           author=ctx.user(),
                           parent=self.siblings(ctx.parents()),
                           child=self.siblings(ctx.children()),
                           changelogtag=showtags,
                           desc=ctx.description(),
                           date=ctx.date(),
                           files=self.listfilediffs(tmpl, ctx.files(), n),
                           rev=ctx.rev(),
                           node=hex(n),
                           tags=self.nodetagsdict(n),
                           inbranch=self.nodeinbranch(ctx),
                           branches=self.nodebranchdict(ctx))

                if count >= self.maxchanges:
                    break

        cl = self.repo.changelog
        parity = paritygen(self.stripecount)

        return tmpl('search',
                    query=query,
                    node=hex(cl.tip()),
                    entries=changelist,
                    archives=self.archivelist("tip"))

    def changeset(self, tmpl, ctx):
        n = ctx.node()
        showtags = self.showtag(tmpl, 'changesettag', n)
        parents = ctx.parents()
        p1 = parents[0].node()

        files = []
        parity = paritygen(self.stripecount)
        for f in ctx.files():
            files.append(tmpl("filenodelink",
                              node=hex(n), file=f,
                              parity=parity.next()))

        def diff(**map):
            yield self.diff(tmpl, p1, n, None)

        return tmpl('changeset',
                    diff=diff,
                    rev=ctx.rev(),
                    node=hex(n),
                    parent=self.siblings(parents),
                    child=self.siblings(ctx.children()),
                    changesettag=showtags,
                    author=ctx.user(),
                    desc=ctx.description(),
                    date=ctx.date(),
                    files=files,
                    archives=self.archivelist(hex(n)),
                    tags=self.nodetagsdict(n),
                    branch=self.nodebranchnodefault(ctx),
                    inbranch=self.nodeinbranch(ctx),
                    branches=self.nodebranchdict(ctx))

    def filelog(self, tmpl, fctx):
        f = fctx.path()
        fl = fctx.filelog()
        count = fl.count()
        pagelen = self.maxshortchanges
        pos = fctx.filerev()
        start = max(0, pos - pagelen + 1)
        end = min(count, start + pagelen)
        pos = end - 1
        parity = paritygen(self.stripecount, offset=start-end)

        def entries(limit=0, **map):
            l = []

            for i in xrange(start, end):
                ctx = fctx.filectx(i)
                n = fl.node(i)

                l.insert(0, {"parity": parity.next(),
                             "filerev": i,
                             "file": f,
                             "node": hex(ctx.node()),
                             "author": ctx.user(),
                             "date": ctx.date(),
                             "rename": self.renamelink(fl, n),
                             "parent": self.siblings(fctx.parents()),
                             "child": self.siblings(fctx.children()),
                             "desc": ctx.description()})

            if limit > 0:
                l = l[:limit]

            for e in l:
                yield e

        nodefunc = lambda x: fctx.filectx(fileid=x)
        nav = revnavgen(pos, pagelen, count, nodefunc)
        return tmpl("filelog", file=f, node=hex(fctx.node()), nav=nav,
                    entries=lambda **x: entries(limit=0, **x),
                    latestentry=lambda **x: entries(limit=1, **x))

    def filerevision(self, tmpl, fctx):
        f = fctx.path()
        text = fctx.data()
        fl = fctx.filelog()
        n = fctx.filenode()
        parity = paritygen(self.stripecount)

        if util.binary(text):
            mt = mimetypes.guess_type(f)[0] or 'application/octet-stream'
            text = '(binary:%s)' % mt

        def lines():
            for lineno, t in enumerate(text.splitlines(1)):
                yield {"line": t,
                       "lineid": "l%d" % (lineno + 1),
                       "linenumber": "% 6d" % (lineno + 1),
                       "parity": parity.next()}

        return tmpl("filerevision",
                    file=f,
                    path=_up(f),
                    text=lines(),
                    rev=fctx.rev(),
                    node=hex(fctx.node()),
                    author=fctx.user(),
                    date=fctx.date(),
                    desc=fctx.description(),
                    branch=self.nodebranchnodefault(fctx),
                    parent=self.siblings(fctx.parents()),
                    child=self.siblings(fctx.children()),
                    rename=self.renamelink(fl, n),
                    permissions=fctx.manifest().flags(f))

    def fileannotate(self, tmpl, fctx):
        f = fctx.path()
        n = fctx.filenode()
        fl = fctx.filelog()
        parity = paritygen(self.stripecount)

        def annotate(**map):
            last = None
            if util.binary(fctx.data()):
                mt = (mimetypes.guess_type(fctx.path())[0]
                      or 'application/octet-stream')
                lines = enumerate([((fctx.filectx(fctx.filerev()), 1),
                                    '(binary:%s)' % mt)])
            else:
                lines = enumerate(fctx.annotate(follow=True, linenumber=True))
            for lineno, ((f, targetline), l) in lines:
                fnode = f.filenode()
                name = self.repo.ui.shortuser(f.user())

                if last != fnode:
                    last = fnode

                yield {"parity": parity.next(),
                       "node": hex(f.node()),
                       "rev": f.rev(),
                       "author": name,
                       "file": f.path(),
                       "targetline": targetline,
                       "line": l,
                       "lineid": "l%d" % (lineno + 1),
                       "linenumber": "% 6d" % (lineno + 1)}

        return tmpl("fileannotate",
                    file=f,
                    annotate=annotate,
                    path=_up(f),
                    rev=fctx.rev(),
                    node=hex(fctx.node()),
                    author=fctx.user(),
                    date=fctx.date(),
                    desc=fctx.description(),
                    rename=self.renamelink(fl, n),
                    branch=self.nodebranchnodefault(fctx),
                    parent=self.siblings(fctx.parents()),
                    child=self.siblings(fctx.children()),
                    permissions=fctx.manifest().flags(f))

    def manifest(self, tmpl, ctx, path):
        mf = ctx.manifest()
        node = ctx.node()

        files = {}
        parity = paritygen(self.stripecount)

        if path and path[-1] != "/":
            path += "/"
        l = len(path)
        abspath = "/" + path

        for f, n in mf.items():
            if f[:l] != path:
                continue
            remain = f[l:]
            if "/" in remain:
                short = remain[:remain.index("/") + 1] # bleah
                files[short] = (f, None)
            else:
                short = os.path.basename(remain)
                files[short] = (f, n)

        if not files:
            raise ErrorResponse(HTTP_NOT_FOUND, 'path not found: ' + path)

        def filelist(**map):
            fl = files.keys()
            fl.sort()
            for f in fl:
                full, fnode = files[f]
                if not fnode:
                    continue

                fctx = ctx.filectx(full)
                yield {"file": full,
                       "parity": parity.next(),
                       "basename": f,
                       "date": fctx.changectx().date(),
                       "size": fctx.size(),
                       "permissions": mf.flags(full)}

        def dirlist(**map):
            fl = files.keys()
            fl.sort()
            for f in fl:
                full, fnode = files[f]
                if fnode:
                    continue

                yield {"parity": parity.next(),
                       "path": "%s%s" % (abspath, f),
                       "basename": f[:-1]}

        return tmpl("manifest",
                    rev=ctx.rev(),
                    node=hex(node),
                    path=abspath,
                    up=_up(abspath),
                    upparity=parity.next(),
                    fentries=filelist,
                    dentries=dirlist,
                    archives=self.archivelist(hex(node)),
                    tags=self.nodetagsdict(node),
                    inbranch=self.nodeinbranch(ctx),
                    branches=self.nodebranchdict(ctx))

    def tags(self, tmpl):
        i = self.repo.tagslist()
        i.reverse()
        parity = paritygen(self.stripecount)

        def entries(notip=False,limit=0, **map):
            count = 0
            for k, n in i:
                if notip and k == "tip":
                    continue
                if limit > 0 and count >= limit:
                    continue
                count = count + 1
                yield {"parity": parity.next(),
                       "tag": k,
                       "date": self.repo.changectx(n).date(),
                       "node": hex(n)}

        return tmpl("tags",
                    node=hex(self.repo.changelog.tip()),
                    entries=lambda **x: entries(False,0, **x),
                    entriesnotip=lambda **x: entries(True,0, **x),
                    latestentry=lambda **x: entries(True,1, **x))

    def summary(self, tmpl):
        i = self.repo.tagslist()
        i.reverse()

        def tagentries(**map):
            parity = paritygen(self.stripecount)
            count = 0
            for k, n in i:
                if k == "tip": # skip tip
                    continue;

                count += 1
                if count > 10: # limit to 10 tags
                    break;

                yield tmpl("tagentry",
                           parity=parity.next(),
                           tag=k,
                           node=hex(n),
                           date=self.repo.changectx(n).date())


        def branches(**map):
            parity = paritygen(self.stripecount)

            b = self.repo.branchtags()
            l = [(-self.repo.changelog.rev(n), n, t) for t, n in b.items()]
            l.sort()

            for r,n,t in l:
                ctx = self.repo.changectx(n)

                yield {'parity': parity.next(),
                       'branch': t,
                       'node': hex(n),
                       'date': ctx.date()}

        def changelist(**map):
            parity = paritygen(self.stripecount, offset=start-end)
            l = [] # build a list in forward order for efficiency
            for i in xrange(start, end):
                ctx = self.repo.changectx(i)
                n = ctx.node()
                hn = hex(n)

                l.insert(0, tmpl(
                   'shortlogentry',
                    parity=parity.next(),
                    author=ctx.user(),
                    desc=ctx.description(),
                    date=ctx.date(),
                    rev=i,
                    node=hn,
                    tags=self.nodetagsdict(n),
                    inbranch=self.nodeinbranch(ctx),
                    branches=self.nodebranchdict(ctx)))

            yield l

        cl = self.repo.changelog
        count = cl.count()
        start = max(0, count - self.maxchanges)
        end = min(count, start + self.maxchanges)

        return tmpl("summary",
                    desc=self.config("web", "description", "unknown"),
                    owner=get_contact(self.config) or "unknown",
                    lastchange=cl.read(cl.tip())[2],
                    tags=tagentries,
                    branches=branches,
                    shortlog=changelist,
                    node=hex(cl.tip()),
                    archives=self.archivelist("tip"))

    def filediff(self, tmpl, fctx):
        n = fctx.node()
        path = fctx.path()
        parents = fctx.parents()
        p1 = parents and parents[0].node() or nullid

        def diff(**map):
            yield self.diff(tmpl, p1, n, [path])

        return tmpl("filediff",
                    file=path,
                    node=hex(n),
                    rev=fctx.rev(),
                    branch=self.nodebranchnodefault(fctx),
                    parent=self.siblings(parents),
                    child=self.siblings(fctx.children()),
                    diff=diff)

    archive_specs = {
        'bz2': ('application/x-tar', 'tbz2', '.tar.bz2', None),
        'gz': ('application/x-tar', 'tgz', '.tar.gz', None),
        'zip': ('application/zip', 'zip', '.zip', None),
        }

    def archive(self, tmpl, req, key, type_):
        reponame = re.sub(r"\W+", "-", os.path.basename(self.reponame))
        cnode = self.repo.lookup(key)
        arch_version = key
        if cnode == key or key == 'tip':
            arch_version = short(cnode)
        name = "%s-%s" % (reponame, arch_version)
        mimetype, artype, extension, encoding = self.archive_specs[type_]
        headers = [
            ('Content-Type', mimetype),
            ('Content-Disposition', 'attachment; filename=%s%s' %
                (name, extension))
        ]
        if encoding:
            headers.append(('Content-Encoding', encoding))
        req.header(headers)
        req.respond(HTTP_OK)
        archival.archive(self.repo, req, cnode, artype, prefix=name)

    # add tags to things
    # tags -> list of changesets corresponding to tags
    # find tag, changeset, file

    def cleanpath(self, path):
        path = path.lstrip('/')
        return util.canonpath(self.repo.root, '', path)

    def changectx(self, req):
        if 'node' in req.form:
            changeid = req.form['node'][0]
        elif 'manifest' in req.form:
            changeid = req.form['manifest'][0]
        else:
            changeid = self.repo.changelog.count() - 1

        try:
            ctx = self.repo.changectx(changeid)
        except RepoError:
            man = self.repo.manifest
            mn = man.lookup(changeid)
            ctx = self.repo.changectx(man.linkrev(mn))

        return ctx

    def filectx(self, req):
        path = self.cleanpath(req.form['file'][0])
        if 'node' in req.form:
            changeid = req.form['node'][0]
        else:
            changeid = req.form['filenode'][0]
        try:
            ctx = self.repo.changectx(changeid)
            fctx = ctx.filectx(path)
        except RepoError:
            fctx = self.repo.filectx(path, fileid=changeid)

        return fctx

    def check_perm(self, req, op, default):
        '''check permission for operation based on user auth.
        return true if op allowed, else false.
        default is policy to use if no config given.'''

        user = req.env.get('REMOTE_USER')

        deny = self.configlist('web', 'deny_' + op)
        if deny and (not user or deny == ['*'] or user in deny):
            return False

        allow = self.configlist('web', 'allow_' + op)
        return (allow and (allow == ['*'] or user in allow)) or default
