# dulwich doesn't return the symref where remote HEAD points, so we monkey
# patch it here
from dulwich.errors import GitProtocolError
from dulwich.protocol import extract_capabilities
from edenscm.mercurial import url, util as hgutil


try:
    from edenscm.mercurial import encoding

    hfsignoreclean = encoding.hfsignoreclean
except AttributeError:
    # compat with hg 3.2.1 and earlier, which doesn't have
    # hfsignoreclean (This was borrowed wholesale from hg 3.2.2.)
    _ignore = [
        unichr(int(x, 16)).encode("utf-8")  # noqa: F821
        for x in "200c 200d 200e 200f 202a 202b 202c 202d 202e "
        "206a 206b 206c 206d 206e 206f feff".split()
    ]
    # verify the next function will work
    assert set([i[0] for i in _ignore]) == set(["\xe2", "\xef"])

    def hfsignoreclean(s):
        """Remove codepoints ignored by HFS+ from s.

        >>> hfsignoreclean(u'.h\u200cg'.encode('utf-8'))
        '.hg'
        >>> hfsignoreclean(u'.h\ufeffg'.encode('utf-8'))
        '.hg'
        """
        if "\xe2" in s or "\xef" in s:
            for c in _ignore:
                s = s.replace(c, "")
        return s


def passwordmgr(ui):
    try:
        realm = hgutil.urlreq.httppasswordmgrwithdefaultrealm()
        return url.passwordmgr(ui, realm)
    except (TypeError, AttributeError):
        # compat with hg < 3.9
        return url.passwordmgr(ui)


def read_pkt_refs(proto):
    server_capabilities = None
    refs = {}
    # Receive refs from server
    for pkt in proto.read_pkt_seq():
        (sha, ref) = pkt.rstrip("\n").split(None, 1)
        if sha == "ERR":
            raise GitProtocolError(ref)
        if server_capabilities is None:
            (ref, server_capabilities) = extract_capabilities(ref)
            symref = "symref=HEAD:"
            for cap in server_capabilities:
                if cap.startswith(symref):
                    sha = cap.replace(symref, "")
        refs[ref] = sha

    if len(refs) == 0:
        return None, set([])
    return refs, set(server_capabilities)


CONFIG_DEFAULTS = {
    "git": {
        "authors": None,
        "blockdotgit": True,
        "blockdothg": True,
        "branch_bookmark_suffix": None,
        "debugextrainmessage": False,  # test only -- do not document this!
        "findcopiesharder": False,
        "intree": None,
        "mindate": None,
        "public": list,
        "renamelimit": 400,
        "similarity": 0,
    },
    "hggit": {"mapsavefrequency": 0, "usephases": False},
}

hasconfigitems = False


def registerconfigs(configitem):
    global hasconfigitems
    hasconfigitems = True
    for section, items in CONFIG_DEFAULTS.iteritems():
        for item, default in items.iteritems():
            configitem(section, item, default=default)


def config(ui, subtype, section, item):
    if subtype == "string":
        subtype = ""
    getconfig = getattr(ui, "config" + subtype)
    if hasconfigitems:
        return getconfig(section, item)
    return getconfig(section, item, CONFIG_DEFAULTS[section][item])
