Test namespace registration using registrar

  $ shorttraceback

  $ newrepo
  $ newext << EOF
  > from edenscm.mercurial import registrar, namespaces
  > namespacepredicate = registrar.namespacepredicate()
  > @namespacepredicate("a", priority=60)
  > def a(_repo):
  >     return namespaces.namespace()
  > @namespacepredicate("b", priority=70)
  > def b(_repo):
  >     return None
  > @namespacepredicate("c", priority=50)
  > def c(_repo):
  >     return namespaces.namespace()
  > EOF

  $ hg debugshell -c "print(list(repo.names))"
  ['bookmarks', 'tags', 'branches', 'c', 'a']

  $ newext << EOF
  > from edenscm.mercurial import registrar, namespaces
  > namespacepredicate = registrar.namespacepredicate()
  > @namespacepredicate("z", priority=99)
  > def z(_repo):
  >     return namespaces.namespace()
  > @namespacepredicate("d", priority=15)
  > def d(_repo):
  >     return namespaces.namespace()
  > EOF
  $ hg debugshell -c "print(list(repo.names))"
  ['bookmarks', 'd', 'tags', 'branches', 'c', 'a', 'z']


Test that not specifying the priority will result in failure to load the
extension.

  $ newext << EOF
  > from edenscm.mercurial import registrar, namespaces
  > namespacepredicate = registrar.namespacepredicate()
  > @namespacepredicate("x", priority=None)
  > def z(_repo):
  >     return namespaces.namespace()
  > EOF


- Run any command to test that the extension loading failed.

  $ hg status
  * failed to import extension *: namespace priority must be specified (glob)
