import cStringIO

from mercurial import util as merc_util

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
        for target in merc_util.sort(self):
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
                    raise merc_util.Abort('invalid externals section name: %s' % line)
                target = line[1:-1]
                if target == '.':
                    target = ''
            elif line.startswith(' '):
                line = line.rstrip('\n')
                if target is None or not line:
                    continue
                self.setdefault(target, []).append(line[1:])
            
