# template-filters.py - common template expansion filters
#
# Copyright 2005-2008 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import cgi, re, os, time, urllib, textwrap
import util, templater

agescales = [("second", 1),
             ("minute", 60),
             ("hour", 3600),
             ("day", 3600 * 24),
             ("week", 3600 * 24 * 7),
             ("month", 3600 * 24 * 30),
             ("year", 3600 * 24 * 365)]

agescales.reverse()

def age(date):
    '''turn a (timestamp, tzoff) tuple into an age string.'''

    def plural(t, c):
        if c == 1:
            return t
        return t + "s"
    def fmt(t, c):
        return "%d %s" % (c, plural(t, c))

    now = time.time()
    then = date[0]
    delta = max(1, int(now - then))

    for t, s in agescales:
        n = delta / s
        if n >= 2 or s == 1:
            return fmt(t, n)

para_re = None
space_re = None

def fill(text, width):
    '''fill many paragraphs.'''
    global para_re, space_re
    if para_re is None:
        para_re = re.compile('(\n\n|\n\\s*[-*]\\s*)', re.M)
        space_re = re.compile(r'  +')

    def findparas():
        start = 0
        while True:
            m = para_re.search(text, start)
            if not m:
                w = len(text)
                while w > start and text[w-1].isspace(): w -= 1
                yield text[start:w], text[w:]
                break
            yield text[start:m.start(0)], m.group(1)
            start = m.end(1)

    return "".join([space_re.sub(' ', textwrap.fill(para, width)) + rest
                    for para, rest in findparas()])

def firstline(text):
    '''return the first line of text'''
    try:
        return text.splitlines(1)[0].rstrip('\r\n')
    except IndexError:
        return ''

def nl2br(text):
    '''replace raw newlines with xhtml line breaks.'''
    return text.replace('\n', '<br/>\n')

def obfuscate(text):
    text = unicode(text, util._encoding, 'replace')
    return ''.join(['&#%d;' % ord(c) for c in text])

def domain(author):
    '''get domain of author, or empty string if none.'''
    f = author.find('@')
    if f == -1: return ''
    author = author[f+1:]
    f = author.find('>')
    if f >= 0: author = author[:f]
    return author

def person(author):
    '''get name of author, or else username.'''
    f = author.find('<')
    if f == -1: return util.shortuser(author)
    return author[:f].rstrip()

def indent(text, prefix):
    '''indent each non-empty line of text after first with prefix.'''
    lines = text.splitlines()
    num_lines = len(lines)
    def indenter():
        for i in xrange(num_lines):
            l = lines[i]
            if i and l.strip():
                yield prefix
            yield l
            if i < num_lines - 1 or text.endswith('\n'):
                yield '\n'
    return "".join(indenter())

def permissions(flags):
    if "l" in flags:
        return "lrwxrwxrwx"
    if "x" in flags:
        return "-rwxr-xr-x"
    return "-rw-r--r--"

def xmlescape(text):
    text = (text
            .replace('&', '&amp;')
            .replace('<', '&lt;')
            .replace('>', '&gt;')
            .replace('"', '&quot;')
            .replace("'", '&#39;')) # &apos; invalid in HTML
    return re.sub('[\x00-\x08\x0B\x0C\x0E-\x1F]', ' ', text)

filters = {
    "addbreaks": nl2br,
    "basename": os.path.basename,
    "age": age,
    "date": lambda x: util.datestr(x),
    "domain": domain,
    "email": util.email,
    "escape": lambda x: cgi.escape(x, True),
    "fill68": lambda x: fill(x, width=68),
    "fill76": lambda x: fill(x, width=76),
    "firstline": firstline,
    "tabindent": lambda x: indent(x, '\t'),
    "hgdate": lambda x: "%d %d" % x,
    "isodate": lambda x: util.datestr(x, '%Y-%m-%d %H:%M %1%2'),
    "isodatesec": lambda x: util.datestr(x, '%Y-%m-%d %H:%M:%S %1%2'),
    "obfuscate": obfuscate,
    "permissions": permissions,
    "person": person,
    "rfc822date": lambda x: util.datestr(x, "%a, %d %b %Y %H:%M:%S %1%2"),
    "rfc3339date": lambda x: util.datestr(x, "%Y-%m-%dT%H:%M:%S%1:%2"),
    "short": lambda x: x[:12],
    "shortdate": util.shortdate,
    "stringify": templater.stringify,
    "strip": lambda x: x.strip(),
    "urlescape": lambda x: urllib.quote(x),
    "user": lambda x: util.shortuser(x),
    "stringescape": lambda x: x.encode('string_escape'),
    "xmlescape": xmlescape,
    }

