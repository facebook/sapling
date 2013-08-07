import os, sys, unittest

_rootdir = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, _rootdir)

from hgsubversion import editor

class TestHelpers(unittest.TestCase):
    def test_filestore(self):
        fs = editor.FileStore(2)
        fs.setfile('a', 'a')
        fs.setfile('b', 'b')
        self.assertEqual('a', fs._data.get('a'))
        self.assertEqual('b', fs._data.get('b'))

        fs.delfile('b')
        self.assertRaises(IOError, lambda: fs.getfile('b'))
        fs.setfile('bb', 'bb')
        self.assertTrue('bb' in fs._files)
        self.assertTrue('bb' not in fs._data)
        self.assertEqual('bb', fs.getfile('bb'))

        fs.delfile('bb')
        self.assertTrue('bb' not in fs._files)
        self.assertEqual([], os.listdir(fs._tempdir))
        self.assertRaises(IOError, lambda: fs.getfile('bb'))

        fs.setfile('bb', 'bb')
        self.assertEqual(1, len(os.listdir(fs._tempdir)))
        fs.popfile('bb')
        self.assertEqual([], os.listdir(fs._tempdir))
        self.assertRaises(editor.EditingError, lambda: fs.getfile('bb'))
