"""
This is Mercurial extension for syntax highlighting in the file
revision view of hgweb.

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

from mercurial import demandimport
demandimport.ignore.extend(['pkgutil', 'pkg_resources', '__main__',])

from mercurial.hgweb import webcommands, webutil, common
from mercurial import util
from mercurial.templatefilters import filters

from pygments import highlight
from pygments.util import ClassNotFound
from pygments.lexers import guess_lexer, guess_lexer_for_filename, TextLexer
from pygments.formatters import HtmlFormatter

SYNTAX_CSS = ('\n<link rel="stylesheet" href="{url}highlightcss" '
              'type="text/css" />')

def pygmentize(field, fctx, style, tmpl):

    # append a <link ...> to the syntax highlighting css
    old_header = ''.join(tmpl('header'))
    if SYNTAX_CSS not in old_header:
        new_header =  old_header + SYNTAX_CSS
        tmpl.cache['header'] = new_header

    text = fctx.data()
    if util.binary(text):
        return

    # To get multi-line strings right, we can't format line-by-line
    try:
        lexer = guess_lexer_for_filename(fctx.path(), text,
                                         encoding=util._encoding)
    except (ClassNotFound, ValueError):
        try:
            lexer = guess_lexer(text, encoding=util._encoding)
        except (ClassNotFound, ValueError):
            lexer = TextLexer(encoding=util._encoding)

    formatter = HtmlFormatter(style=style, encoding=util._encoding)

    colorized = highlight(text, lexer, formatter)
    # strip wrapping div
    colorized = colorized[:colorized.find('\n</pre>')]
    colorized = colorized[colorized.find('<pre>')+5:]
    coloriter = iter(colorized.splitlines())

    filters['colorize'] = lambda x: coloriter.next()

    oldl = tmpl.cache[field]
    newl = oldl.replace('line|escape', 'line|colorize')
    tmpl.cache[field] = newl

web_filerevision = webcommands._filerevision
web_annotate = webcommands.annotate

def filerevision_highlight(web, tmpl, fctx):
    style = web.config('web', 'pygments_style', 'colorful')
    pygmentize('fileline', fctx, style, tmpl)
    return web_filerevision(web, tmpl, fctx)

def annotate_highlight(web, req, tmpl):
    fctx = webutil.filectx(web.repo, req)
    style = web.config('web', 'pygments_style', 'colorful')
    pygmentize('annotateline', fctx, style, tmpl)
    return web_annotate(web, req, tmpl)

def generate_css(web, req, tmpl):
    pg_style = web.config('web', 'pygments_style', 'colorful')
    fmter = HtmlFormatter(style = pg_style)
    req.respond(common.HTTP_OK, 'text/css')
    return ['/* pygments_style = %s */\n\n' % pg_style, fmter.get_style_defs('')]


# monkeypatch in the new version

webcommands._filerevision = filerevision_highlight
webcommands.annotate = annotate_highlight
webcommands.highlightcss = generate_css
webcommands.__all__.append('highlightcss')
