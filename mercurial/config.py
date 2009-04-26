from i18n import _
import re, error

class sortdict(dict):
    'a simple append-only sorted dictionary'
    def __init__(self, data=None):
        self._list = []
        if data:
            if hasattr(data, '_list'):
                self._list = list(data._list)
            self.update(data)
    def copy(self):
        return sortdict(self)
    def __setitem__(self, key, val):
        if key in self:
            self._list.remove(key)
        self._list.append(key)
        dict.__setitem__(self, key, val)
    def __iter__(self):
        return self._list.__iter__()
    def update(self, src):
        for k in src:
            self[k] = src[k]
    def items(self):
        return [(k,self[k]) for k in self._list]

class config:
    def __init__(self, data=None):
        self._data = {}
        if data:
            for k in data._data:
                self._data[k] = data[k].copy()
    def copy(self):
        return config(self)
    def __contains__(self, section):
        return section in self._data
    def update(self, src, sections=None):
        if not sections:
            sections = src.sections()
        for s in sections:
            if s not in src:
                continue
            if s not in self:
                self._data[s] = sortdict()
            for k in src._data[s]:
                self._data[s][k] = src._data[s][k]
    def get(self, section, item, default=None):
        return self._data.get(section, {}).get(item, (default, ""))[0]
    def getsource(self, section, item):
        return self._data.get(section, {}).get(item, (None, ""))[1]
    def sections(self):
        return sorted(self._data.keys())
    def items(self, section):
        return [(k, v[0]) for k,v in self._data.get(section, {}).items()]
    def set(self, section, item, value, source=""):
        if section not in self:
            self._data[section] = sortdict()
        self._data[section][item] = (value, source)

    def read(self, path, fp=None):
        sectionre = re.compile(r'\[([^\[]+)\]')
        itemre = re.compile(r'([^=\s]+)\s*=\s*(.*)')
        contre = re.compile(r'\s+(\S.*)')
        emptyre = re.compile(r'(;|#|\s*$)')
        section = ""
        item = None
        line = 0
        cont = 0

        if not fp:
            fp = open(path)

        for l in fp:
            line += 1
            if cont:
                m = contre.match(l)
                if m:
                    v = self.get(section, item) + "\n" + m.group(1)
                    self.set(section, item, v, "%s:%d" % (path, line))
                    continue
                item = None
            if emptyre.match(l):
                continue
            m = sectionre.match(l)
            if m:
                section = m.group(1)
                if section not in self:
                    self._data[section] = sortdict()
                continue
            m = itemre.match(l)
            if m:
                item = m.group(1)
                self.set(section, item, m.group(2), "%s:%d" % (path, line))
                cont = 1
                continue
            raise error.ConfigError(_('config error at %s:%d: \'%s\'')
                                    % (path, line, l.rstrip()))
