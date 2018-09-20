Import bookmarkstore and test adding bookmarks
  >>> from mercurial.rust import bookmarkstore
  >>> from mercurial import node
  >>> bm_store = bookmarkstore.bookmarkstore()
  >>> print(bm_store.lookup_bookmark("not-real"))
  None
  >>> bm_store.add_bookmark("test", node.nullid)
  >>> bm_store.lookup_bookmark("test")
  '\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00'
  >>> bm_store.add_bookmark("test", node.bin("1" * 40))
  >>> bm_store.lookup_bookmark("test")
  '\x11\x11\x11\x11\x11\x11\x11\x11\x11\x11\x11\x11\x11\x11\x11\x11\x11\x11\x11\x11'
  >>> bm_store.remove_bookmark("test")

Test multiple bookmarks pointing to the same node
  >>> from mercurial.rust import bookmarkstore
  >>> from mercurial import node
  >>> bm_store = bookmarkstore.bookmarkstore()
  >>> print(bm_store.lookup_node(node.bin("2" * 40)))
  None
  >>> bm_store.add_bookmark("test", node.bin("2" * 40))
  >>> bm_store.add_bookmark("test2", node.bin("2" * 40))
  >>> bm_store.lookup_node(node.bin("2" * 40))
  ['test', 'test2']
  >>> bm_store.remove_bookmark("test2")
  >>> bm_store.lookup_node(node.bin("2" * 40))
  ['test']

Test loading from bookmark file
  >>> from tempfile import NamedTemporaryFile
  >>> from mercurial.rust import bookmarkstore
  >>> with NamedTemporaryFile() as f:
  ...   f.write("{} test1\n".format('1' * 40))
  ...   f.flush()
  ...   bm_store = bookmarkstore.bookmarkstore(f.name)
  ...   bm_store.lookup_bookmark('test1')
  '\x11\x11\x11\x11\x11\x11\x11\x11\x11\x11\x11\x11\x11\x11\x11\x11\x11\x11\x11\x11'

Test malformed bookmark file
  >>> from tempfile import NamedTemporaryFile
  >>> from mercurial.rust import bookmarkstore
  >>> with NamedTemporaryFile() as f:
  ...   f.write("{} test1\n".format('z' * 40))
  ...   f.flush()
  ...   bm_store = bookmarkstore.bookmarkstore(f.name)
  IOError('malformed bookmark file at line 1: zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz test1',)

Test flushing bookmarkstore to file
  >>> from tempfile import NamedTemporaryFile
  >>> from mercurial.rust import bookmarkstore
  >>> from mercurial import node
  >>> bm_store = bookmarkstore.bookmarkstore()
  >>> bm_store.add_bookmark("test", node.bin("1" * 40))
  >>> with NamedTemporaryFile() as f:
  ...   bm_store.flush(f.name)
  ...   open(f.name).readlines()
  ['1111111111111111111111111111111111111111 test']
