try:
    from mercurial import encoding
    hfsignoreclean = encoding.hfsignoreclean
except AttributeError:
    # compat with hg 3.2.1 and earlier, which doesn't have
    # hfsignoreclean (This was borrowed wholesale from hg 3.2.2.)
    _ignore = [unichr(int(x, 16)).encode("utf-8") for x in
               "200c 200d 200e 200f 202a 202b 202c 202d 202e "
               "206a 206b 206c 206d 206e 206f feff".split()]
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
                s = s.replace(c, '')
        return s
