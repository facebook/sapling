"""a mercurial extension for syntax highlighting in hgweb

It depends on the pygments syntax highlighting library:
http://pygments.org/

To enable the extension add this to hgrc:

[extensions]
hgext.highlight =

There is a single configuration option:

[web]
pygments_style = <style>

The default is 'colorful'.

-- Adam Hupp <adam@hupp.org>
"""

import highlight
from mercurial.hgweb import webcommands, webutil, common

web_filerevision = webcommands._filerevision
web_annotate = webcommands.annotate

def filerevision_highlight(web, tmpl, fctx):
    style = web.config('web', 'pygments_style', 'colorful')
    highlight.pygmentize('fileline', fctx, style, tmpl)
    return web_filerevision(web, tmpl, fctx)

def annotate_highlight(web, req, tmpl):
    fctx = webutil.filectx(web.repo, req)
    style = web.config('web', 'pygments_style', 'colorful')
    highlight.pygmentize('annotateline', fctx, style, tmpl)
    return web_annotate(web, req, tmpl)

def generate_css(web, req, tmpl):
    pg_style = web.config('web', 'pygments_style', 'colorful')
    fmter = highlight.HtmlFormatter(style = pg_style)
    req.respond(common.HTTP_OK, 'text/css')
    return ['/* pygments_style = %s */\n\n' % pg_style, fmter.get_style_defs('')]


# monkeypatch in the new version

webcommands._filerevision = filerevision_highlight
webcommands.annotate = annotate_highlight
webcommands.highlightcss = generate_css
webcommands.__all__.append('highlightcss')
