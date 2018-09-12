Test namespace registration using registrar

  $ shorttraceback

  $ newrepo
  $ newext << EOF
  > from mercurial import registrar, namespaces
  > namespacepredicate = registrar.namespacepredicate()
  > @namespacepredicate("a", priority=60)
  > def a(_repo):
  >     return namespaces.namespace("a")
  > @namespacepredicate("b", priority=70)
  > def b(_repo):
  >     return None
  > @namespacepredicate("c", priority=50)
  > def c(_repo):
  >     return namespaces.namespace("c")
  > EOF

  $ hg debugshell -c "print(list(repo.names))"
  ['bookmarks', 'tags', 'branches', 'c', 'a']

  $ newext << EOF
  > from mercurial import registrar, namespaces
  > namespacepredicate = registrar.namespacepredicate()
  > @namespacepredicate("z", priority=99)
  > def z(_repo):
  >     return namespaces.namespace("z")
  > @namespacepredicate("d", priority=15)
  > def d(_repo):
  >     return namespaces.namespace("d")
  > EOF
  $ hg debugshell -c "print(list(repo.names))"
  ['bookmarks', 'd', 'tags', 'branches', 'c', 'a', 'z']

