import random, sys
import unittest

sys.path.append('../')

import toposort

class Ob:
    def __init__(self, eyedee, parents):
        self._id = eyedee
        self._parents = parents
        
    def id(self):
        return self._id

    def parents(self):
        return self._parents


class TestTopoSorting(unittest.TestCase):

    def testsort(self):
        data = {
         'f' : Ob('f', ['d', 'e']),
         'd' : Ob('d', ['b']),
         'e' : Ob('e', ['c', 'g']),
         'g' : Ob('g', ['c']),
         'c' : Ob('c', ['b', 'h']),
         'b' : Ob('b', ['a']),
         'h' : Ob('h', ['a']),
         'a' : Ob('a', []),
        }
        d = toposort.TopoSort(data).items()
        sort = ['a', 'b', 'd', 'h', 'c', 'g', 'e', 'f']
        self.assertEquals(d, sort)

if __name__ == '__main__':
    unittest.main()