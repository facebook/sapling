import os
from mercurial import filelog, util

def getcachekey(reponame, file, id):
    pathhash = util.sha1(file).hexdigest()
    return os.path.join(reponame, pathhash[:2], pathhash[2:], id)

def getlocalkey(file, id):
    pathhash = util.sha1(file).hexdigest()
    return os.path.join(pathhash, id)

def createrevlogtext(text, copyfrom=None, copyrev=None):
    """returns a string that matches the revlog contents in a
    traditional revlog
    """
    meta = {}
    if copyfrom or text.startswith('\1\n'):
        if copyfrom:
            meta['copy'] = copyfrom
            meta['copyrev'] = copyrev
        text = filelog.packmeta(meta, text)

    return text

def parsemeta(text):
    meta, size = filelog.parsemeta(text)
    if text.startswith('\1\n'):
        s = text.index('\1\n', 2)
        text = text[s + 2:]
    return meta or {}, text

def parsesize(raw):
    try:
        index = raw.index('\0')
        size = int(raw[:index])
    except ValueError:
        raise Exception("corrupt cache data")
    return index, size

def ancestormap(raw):
    index, size = parsesize(raw)
    start = index + 1 + size

    mapping = {}
    while start < len(raw):
        divider = raw.index('\0', start + 80)

        currentnode = raw[start:(start + 20)]
        p1 = raw[(start + 20):(start + 40)]
        p2 = raw[(start + 40):(start + 60)]
        linknode = raw[(start + 60):(start + 80)]
        copyfrom = raw[(start + 80):divider]

        mapping[currentnode] = (p1, p2, linknode, copyfrom)
        start = divider + 1

    return mapping

def readfile(path):
    f = open(path, "r")
    try:
        result = f.read()

        # we should never have empty files
        if not result:
            os.remove(path)
            raise IOError("empty file: %s" % path)

        return result
    finally:
        f.close()

def writefile(path, content, readonly=False):
    dirname = os.path.dirname(path)
    if not os.path.exists(dirname):
        try:
            os.makedirs(dirname)
        except OSError, ex:
            if ex.errno != errno.EEXIST:
                raise

    # atomictempfile doesn't pick up the fact that we changed the umask, so we
    # need to set it manually.
    f = util.atomictempfile(path, 'w', createmode=~os.umask(0))
    try:
        f.write(content)
    finally:
        f.close()

    if readonly:
        os.chmod(path, 0o444)
