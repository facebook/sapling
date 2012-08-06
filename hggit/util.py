"""Compatability functions for old Mercurial versions."""
import ordereddict

def progress(ui, *args, **kwargs):
    """Shim for progress on hg < 1.4. Remove when 1.3 is dropped."""
    getattr(ui, 'progress', lambda *x, **kw: None)(*args, **kwargs)

def parse_hgsub(lines):
    """Fills OrderedDict with hgsub file content passed as list of lines"""
    rv = ordereddict.OrderedDict()
    for l in lines:
        ls = l.strip();
        if not ls or ls[0] == '#': continue
        name, value = l.split('=', 1)
        rv[name.strip()] = value.strip()
    return rv

def serialize_hgsub(data):
    """Produces a string from OrderedDict hgsub content"""
    return ''.join(['%s = %s\n' % (n,v) for n,v in data.iteritems()])

def parse_hgsubstate(lines):
    """Fills OrderedDict with hgsubtate file content passed as list of lines"""
    rv = ordereddict.OrderedDict()
    for l in lines:
        ls = l.strip();
        if not ls or ls[0] == '#': continue
        value, name = l.split(' ', 1)
        rv[name.strip()] = value.strip()
    return rv

def serialize_hgsubstate(data):
    """Produces a string from OrderedDict hgsubstate content"""
    return ''.join(['%s %s\n' % (data[n], n) for n in sorted(data)])

