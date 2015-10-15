# highlight - syntax highlighting in hgweb, based on Pygments
#
#  Copyright 2008, 2009 Patrick Mezard <pmezard@gmail.com> and others
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
#
# The original module was split in an interface and an implementation
# file to defer pygments loading and speedup extension setup.

"""syntax highlighting for hgweb (requires Pygments)

It depends on the Pygments syntax highlighting library:
http://pygments.org/

There are the following configuration options::

  [web]
  pygments_style = <style> (default: colorful)
  highlightfiles = <fileset> (default: size('<5M'))
  highlightonlymatchfilename = <bool> (default False)

``highlightonlymatchfilename`` will only highlight files if their type could
be identified by their filename. When this is not enabled (the default),
Pygments will try very hard to identify the file type from content and any
match (even matches with a low confidence score) will be used.
"""

import highlight
from mercurial.hgweb import webcommands, webutil, common
from mercurial import extensions, encoding, fileset
# Note for extension authors: ONLY specify testedwith = 'internal' for
# extensions which SHIP WITH MERCURIAL. Non-mainline extensions should
# be specifying the version(s) of Mercurial they are tested with, or
# leave the attribute unspecified.
testedwith = 'internal'

def pygmentize(web, field, fctx, tmpl):
    style = web.config('web', 'pygments_style', 'colorful')
    expr = web.config('web', 'highlightfiles', "size('<5M')")
    filenameonly = web.configbool('web', 'highlightonlymatchfilename', False)

    ctx = fctx.changectx()
    tree = fileset.parse(expr)
    mctx = fileset.matchctx(ctx, subset=[fctx.path()], status=None)
    if fctx.path() in fileset.getset(mctx, tree):
        highlight.pygmentize(field, fctx, style, tmpl,
                guessfilenameonly=filenameonly)

def filerevision_highlight(orig, web, req, tmpl, fctx):
    mt = ''.join(tmpl('mimetype', encoding=encoding.encoding))
    # only pygmentize for mimetype containing 'html' so we both match
    # 'text/html' and possibly 'application/xhtml+xml' in the future
    # so that we don't have to touch the extension when the mimetype
    # for a template changes; also hgweb optimizes the case that a
    # raw file is sent using rawfile() and doesn't call us, so we
    # can't clash with the file's content-type here in case we
    # pygmentize a html file
    if 'html' in mt:
        pygmentize(web, 'fileline', fctx, tmpl)

    return orig(web, req, tmpl, fctx)

def annotate_highlight(orig, web, req, tmpl):
    mt = ''.join(tmpl('mimetype', encoding=encoding.encoding))
    if 'html' in mt:
        fctx = webutil.filectx(web.repo, req)
        pygmentize(web, 'annotateline', fctx, tmpl)

    return orig(web, req, tmpl)

def generate_css(web, req, tmpl):
    pg_style = web.config('web', 'pygments_style', 'colorful')
    fmter = highlight.HtmlFormatter(style=pg_style)
    req.respond(common.HTTP_OK, 'text/css')
    return ['/* pygments_style = %s */\n\n' % pg_style,
            fmter.get_style_defs('')]

def extsetup():
    # monkeypatch in the new version
    extensions.wrapfunction(webcommands, '_filerevision',
                            filerevision_highlight)
    extensions.wrapfunction(webcommands, 'annotate', annotate_highlight)
    webcommands.highlightcss = generate_css
    webcommands.__all__.append('highlightcss')
