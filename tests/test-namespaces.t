Test namespace registration using registrar

  $ shorttraceback
  $ newrepo
  $ newext << EOF
  > from mercurial import registrar, namespaces
  > namespacepredicate = registrar.namespacepredicate()
  > @namespacepredicate("a", after=["b", "v"])
  > def a(_repo):
  >     return namespaces.namespace("a")
  > @namespacepredicate("b", after=["c"])
  > def b(_repo):
  >     return None
  > @namespacepredicate("c", after=["d"])
  > def c(_repo):
  >     return namespaces.namespace("c")
  > EOF

  $ hg debugshell -c "print(list(repo.names))"
  ['bookmarks', 'tags', 'branches', 'c', 'a']

  $ newext << EOF
  > from mercurial import registrar, namespaces
  > namespacepredicate = registrar.namespacepredicate()
  > @namespacepredicate("z", after=["a"])
  > def z(_repo):
  >     return namespaces.namespace("z")
  > @namespacepredicate("d")
  > def d(_repo):
  >     return namespaces.namespace("d")
  > EOF
  $ hg debugshell -c "print(list(repo.names))"
  ['bookmarks', 'tags', 'branches', 'd', 'c', 'a', 'z']

  $ newext << EOF
  > from mercurial import registrar, namespaces
  > namespacepredicate = registrar.namespacepredicate()
  > @namespacepredicate("u", after=["a"])
  > def u(_repo):
  >     return namespaces.namespace("u")
  > @namespacepredicate("v", after=["u"])
  > def v(_repo):
  >     return namespaces.namespace("v")
  > EOF
  $ hg debugshell -c "print(list(repo.names))"
  ProgrammingError: namespace order constraints cannot be satisfied: a, b, u, v, z
  [255]
