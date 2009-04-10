import cStringIO

from mercurial import util as hgutil

class externalsfile(dict):
    """Map svn directories to lists of externals entries.
    """
    def __init__(self):
        super(externalsfile, self).__init__()
        self.encoding = 'utf-8'

    def __setitem__(self, key, value):
        if value is None:
            value = []
        elif isinstance(value, basestring):
            value = value.splitlines()
        if key == '.':
            key = ''
        if not value:
            if key in self:
                del self[key]
        else:
            super(externalsfile, self).__setitem__(key, value)

    def write(self):
        fp = cStringIO.StringIO()
        for target in hgutil.sort(self):
            lines = self[target]
            if not lines:
                continue
            if not target:
                target = '.'
            fp.write('[%s]\n' % target)
            for l in lines:
                l = ' ' + l + '\n'
                fp.write(l)
        return fp.getvalue()

    def read(self, data):
        self.clear()
        fp = cStringIO.StringIO(data)
        dirs = {}
        target = None
        for line in fp.readlines():
            if not line.strip():
                continue
            if line.startswith('['):
                line = line.strip()
                if line[-1] != ']':
                    raise hgutil.Abort('invalid externals section name: %s' % line)
                target = line[1:-1]
                if target == '.':
                    target = ''
            elif line.startswith(' '):
                line = line.rstrip('\n')
                if target is None or not line:
                    continue
                self.setdefault(target, []).append(line[1:])
            
def diff(ext1, ext2):
    """Compare 2 externalsfile and yield tuples like (dir, value1, value2)
    where value1 is the external value in ext1 for dir or None, and
    value2 the same in ext2.
    """
    for d in ext1:
        if d not in ext2:
            yield d, '\n'.join(ext1[d]), None
        elif ext1[d] != ext2[d]:
            yield d, '\n'.join(ext1[d]), '\n'.join(ext2[d])
    for d in ext2:
        if d not in ext1:
            yield d, None, '\n'.join(ext2[d])
