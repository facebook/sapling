# template-filters.py - common template expansion filters
#
# Copyright 2005-2008 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import cgi
import os
import re
import time

from . import (
    encoding,
    hbisect,
    node,
    registrar,
    templatekw,
    util,
)

urlerr = util.urlerr
urlreq = util.urlreq

# filters are callables like:
#   fn(obj)
# with:
#   obj - object to be filtered (text, date, list and so on)
filters = {}

templatefilter = registrar.templatefilter(filters)

@templatefilter('addbreaks')
def addbreaks(text):
    """Any text. Add an XHTML "<br />" tag before the end of
    every line except the last.
    """
    return text.replace('\n', '<br/>\n')

agescales = [("year", 3600 * 24 * 365, 'Y'),
             ("month", 3600 * 24 * 30, 'M'),
             ("week", 3600 * 24 * 7, 'W'),
             ("day", 3600 * 24, 'd'),
             ("hour", 3600, 'h'),
             ("minute", 60, 'm'),
             ("second", 1, 's')]

@templatefilter('age')
def age(date, abbrev=False):
    """Date. Returns a human-readable date/time difference between the
    given date/time and the current date/time.
    """

    def plural(t, c):
        if c == 1:
            return t
        return t + "s"
    def fmt(t, c, a):
        if abbrev:
            return "%d%s" % (c, a)
        return "%d %s" % (c, plural(t, c))

    now = time.time()
    then = date[0]
    future = False
    if then > now:
        future = True
        delta = max(1, int(then - now))
        if delta > agescales[0][1] * 30:
            return 'in the distant future'
    else:
        delta = max(1, int(now - then))
        if delta > agescales[0][1] * 2:
            return util.shortdate(date)

    for t, s, a in agescales:
        n = delta // s
        if n >= 2 or s == 1:
            if future:
                return '%s from now' % fmt(t, n, a)
            return '%s ago' % fmt(t, n, a)

@templatefilter('basename')
def basename(path):
    """Any text. Treats the text as a path, and returns the last
    component of the path after splitting by the path separator
    (ignoring trailing separators). For example, "foo/bar/baz" becomes
    "baz" and "foo/bar//" becomes "bar".
    """
    return os.path.basename(path)

@templatefilter('count')
def count(i):
    """List or text. Returns the length as an integer."""
    return len(i)

@templatefilter('domain')
def domain(author):
    """Any text. Finds the first string that looks like an email
    address, and extracts just the domain component. Example: ``User
    <user@example.com>`` becomes ``example.com``.
    """
    f = author.find('@')
    if f == -1:
        return ''
    author = author[f + 1:]
    f = author.find('>')
    if f >= 0:
        author = author[:f]
    return author

@templatefilter('email')
def email(text):
    """Any text. Extracts the first string that looks like an email
    address. Example: ``User <user@example.com>`` becomes
    ``user@example.com``.
    """
    return util.email(text)

@templatefilter('escape')
def escape(text):
    """Any text. Replaces the special XML/XHTML characters "&", "<"
    and ">" with XML entities, and filters out NUL characters.
    """
    return cgi.escape(text.replace('\0', ''), True)

para_re = None
space_re = None

def fill(text, width, initindent='', hangindent=''):
    '''fill many paragraphs with optional indentation.'''
    global para_re, space_re
    if para_re is None:
        para_re = re.compile('(\n\n|\n\\s*[-*]\\s*)', re.M)
        space_re = re.compile(r'  +')

    def findparas():
        start = 0
        while True:
            m = para_re.search(text, start)
            if not m:
                uctext = unicode(text[start:], encoding.encoding)
                w = len(uctext)
                while 0 < w and uctext[w - 1].isspace():
                    w -= 1
                yield (uctext[:w].encode(encoding.encoding),
                       uctext[w:].encode(encoding.encoding))
                break
            yield text[start:m.start(0)], m.group(1)
            start = m.end(1)

    return "".join([util.wrap(space_re.sub(' ', util.wrap(para, width)),
                              width, initindent, hangindent) + rest
                    for para, rest in findparas()])

@templatefilter('fill68')
def fill68(text):
    """Any text. Wraps the text to fit in 68 columns."""
    return fill(text, 68)

@templatefilter('fill76')
def fill76(text):
    """Any text. Wraps the text to fit in 76 columns."""
    return fill(text, 76)

@templatefilter('firstline')
def firstline(text):
    """Any text. Returns the first line of text."""
    try:
        return text.splitlines(True)[0].rstrip('\r\n')
    except IndexError:
        return ''

@templatefilter('hex')
def hexfilter(text):
    """Any text. Convert a binary Mercurial node identifier into
    its long hexadecimal representation.
    """
    return node.hex(text)

@templatefilter('hgdate')
def hgdate(text):
    """Date. Returns the date as a pair of numbers: "1157407993
    25200" (Unix timestamp, timezone offset).
    """
    return "%d %d" % text

@templatefilter('isodate')
def isodate(text):
    """Date. Returns the date in ISO 8601 format: "2009-08-18 13:00
    +0200".
    """
    return util.datestr(text, '%Y-%m-%d %H:%M %1%2')

@templatefilter('isodatesec')
def isodatesec(text):
    """Date. Returns the date in ISO 8601 format, including
    seconds: "2009-08-18 13:00:13 +0200". See also the rfc3339date
    filter.
    """
    return util.datestr(text, '%Y-%m-%d %H:%M:%S %1%2')

def indent(text, prefix):
    '''indent each non-empty line of text after first with prefix.'''
    lines = text.splitlines()
    num_lines = len(lines)
    endswithnewline = text[-1:] == '\n'
    def indenter():
        for i in xrange(num_lines):
            l = lines[i]
            if i and l.strip():
                yield prefix
            yield l
            if i < num_lines - 1 or endswithnewline:
                yield '\n'
    return "".join(indenter())

@templatefilter('json')
def json(obj):
    if obj is None or obj is False or obj is True:
        return {None: 'null', False: 'false', True: 'true'}[obj]
    elif isinstance(obj, int) or isinstance(obj, float):
        return str(obj)
    elif isinstance(obj, str):
        return '"%s"' % encoding.jsonescape(obj, paranoid=True)
    elif util.safehasattr(obj, 'keys'):
        out = []
        for k, v in sorted(obj.iteritems()):
            s = '%s: %s' % (json(k), json(v))
            out.append(s)
        return '{' + ', '.join(out) + '}'
    elif util.safehasattr(obj, '__iter__'):
        out = []
        for i in obj:
            out.append(json(i))
        return '[' + ', '.join(out) + ']'
    elif util.safehasattr(obj, '__call__'):
        return json(obj())
    else:
        raise TypeError('cannot encode type %s' % obj.__class__.__name__)

@templatefilter('lower')
def lower(text):
    """Any text. Converts the text to lowercase."""
    return encoding.lower(text)

@templatefilter('nonempty')
def nonempty(str):
    """Any text. Returns '(none)' if the string is empty."""
    return str or "(none)"

@templatefilter('obfuscate')
def obfuscate(text):
    """Any text. Returns the input text rendered as a sequence of
    XML entities.
    """
    text = unicode(text, encoding.encoding, 'replace')
    return ''.join(['&#%d;' % ord(c) for c in text])

@templatefilter('permissions')
def permissions(flags):
    if "l" in flags:
        return "lrwxrwxrwx"
    if "x" in flags:
        return "-rwxr-xr-x"
    return "-rw-r--r--"

@templatefilter('person')
def person(author):
    """Any text. Returns the name before an email address,
    interpreting it as per RFC 5322.

    >>> person('foo@bar')
    'foo'
    >>> person('Foo Bar <foo@bar>')
    'Foo Bar'
    >>> person('"Foo Bar" <foo@bar>')
    'Foo Bar'
    >>> person('"Foo \"buz\" Bar" <foo@bar>')
    'Foo "buz" Bar'
    >>> # The following are invalid, but do exist in real-life
    ...
    >>> person('Foo "buz" Bar <foo@bar>')
    'Foo "buz" Bar'
    >>> person('"Foo Bar <foo@bar>')
    'Foo Bar'
    """
    if '@' not in author:
        return author
    f = author.find('<')
    if f != -1:
        return author[:f].strip(' "').replace('\\"', '"')
    f = author.find('@')
    return author[:f].replace('.', ' ')

@templatefilter('revescape')
def revescape(text):
    """Any text. Escapes all "special" characters, except @.
    Forward slashes are escaped twice to prevent web servers from prematurely
    unescaping them. For example, "@foo bar/baz" becomes "@foo%20bar%252Fbaz".
    """
    return urlreq.quote(text, safe='/@').replace('/', '%252F')

@templatefilter('rfc3339date')
def rfc3339date(text):
    """Date. Returns a date using the Internet date format
    specified in RFC 3339: "2009-08-18T13:00:13+02:00".
    """
    return util.datestr(text, "%Y-%m-%dT%H:%M:%S%1:%2")

@templatefilter('rfc822date')
def rfc822date(text):
    """Date. Returns a date using the same format used in email
    headers: "Tue, 18 Aug 2009 13:00:13 +0200".
    """
    return util.datestr(text, "%a, %d %b %Y %H:%M:%S %1%2")

@templatefilter('short')
def short(text):
    """Changeset hash. Returns the short form of a changeset hash,
    i.e. a 12 hexadecimal digit string.
    """
    return text[:12]

@templatefilter('shortbisect')
def shortbisect(text):
    """Any text. Treats `text` as a bisection status, and
    returns a single-character representing the status (G: good, B: bad,
    S: skipped, U: untested, I: ignored). Returns single space if `text`
    is not a valid bisection status.
    """
    return hbisect.shortlabel(text) or ' '

@templatefilter('shortdate')
def shortdate(text):
    """Date. Returns a date like "2006-09-18"."""
    return util.shortdate(text)

@templatefilter('splitlines')
def splitlines(text):
    """Any text. Split text into a list of lines."""
    return templatekw.showlist('line', text.splitlines(), 'lines')

@templatefilter('stringescape')
def stringescape(text):
    return text.encode('string_escape')

@templatefilter('stringify')
def stringify(thing):
    """Any type. Turns the value into text by converting values into
    text and concatenating them.
    """
    if util.safehasattr(thing, '__iter__') and not isinstance(thing, str):
        return "".join([stringify(t) for t in thing if t is not None])
    if thing is None:
        return ""
    return str(thing)

@templatefilter('stripdir')
def stripdir(text):
    """Treat the text as path and strip a directory level, if
    possible. For example, "foo" and "foo/bar" becomes "foo".
    """
    dir = os.path.dirname(text)
    if dir == "":
        return os.path.basename(text)
    else:
        return dir

@templatefilter('tabindent')
def tabindent(text):
    """Any text. Returns the text, with every non-empty line
    except the first starting with a tab character.
    """
    return indent(text, '\t')

@templatefilter('upper')
def upper(text):
    """Any text. Converts the text to uppercase."""
    return encoding.upper(text)

@templatefilter('urlescape')
def urlescape(text):
    """Any text. Escapes all "special" characters. For example,
    "foo bar" becomes "foo%20bar".
    """
    return urlreq.quote(text)

@templatefilter('user')
def userfilter(text):
    """Any text. Returns a short representation of a user name or email
    address."""
    return util.shortuser(text)

@templatefilter('emailuser')
def emailuser(text):
    """Any text. Returns the user portion of an email address."""
    return util.emailuser(text)

@templatefilter('utf8')
def utf8(text):
    """Any text. Converts from the local character encoding to UTF-8."""
    return encoding.fromlocal(text)

@templatefilter('xmlescape')
def xmlescape(text):
    text = (text
            .replace('&', '&amp;')
            .replace('<', '&lt;')
            .replace('>', '&gt;')
            .replace('"', '&quot;')
            .replace("'", '&#39;')) # &apos; invalid in HTML
    return re.sub('[\x00-\x08\x0B\x0C\x0E-\x1F]', ' ', text)

def websub(text, websubtable):
    """:websub: Any text. Only applies to hgweb. Applies the regular
    expression replacements defined in the websub section.
    """
    if websubtable:
        for regexp, format in websubtable:
            text = regexp.sub(format, text)
    return text

def loadfilter(ui, extname, registrarobj):
    """Load template filter from specified registrarobj
    """
    for name, func in registrarobj._table.iteritems():
        filters[name] = func

# tell hggettext to extract docstrings from these functions:
i18nfunctions = filters.values()
