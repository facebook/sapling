#chg-compatible

Terse output:

  $ hg debugbundle "$TESTDIR/bundles/bx2rebase.hg"
  Stream params: {preloadmanifests: 58035ddbe291fb4575460ad41f75f34494c48649}
  replycaps -- {}
  b2x:commonheads -- {}
  b2x:rebasepackpart -- {cache: False, category: manifests, version: 1}
  b2x:rebase -- {cgversion: 02, obsmarkerversions: 0\x001, onto: master} (esc)
      e6fd2b5fc895fb648cad93b3507c3260a775a762
      fb664400597ce0918c57f5e755eab6d4f2afa725
  pushkey -- {key: fb664400597ce0918c57f5e755eab6d4f2afa725, namespace: phases, new: 0, old: 1}
  pushkey -- {key: master, namespace: bookmarks, new: fb664400597ce0918c57f5e755eab6d4f2afa725, old: 4861b3248c40901dc09c91aae80750c2b802aa7e}

Verbose output:

  $ hg debugbundle --all "$TESTDIR/bundles/bx2rebase.hg"
  Stream params: {preloadmanifests: 58035ddbe291fb4575460ad41f75f34494c48649}
  replycaps -- {}
  b2x:commonheads -- {}
  b2x:rebasepackpart -- {cache: False, category: manifests, version: 1}
  b2x:rebase -- {cgversion: 02, obsmarkerversions: 0\x001, onto: master} (esc)
      format: id, p1, p2, cset, delta base, len(delta)
  
      changelog
      e6fd2b5fc895fb648cad93b3507c3260a775a762 69ad5a6495bf24f9afd13a7a3ca19074f5d222ca 0000000000000000000000000000000000000000 0000000000000000000000000000000000000000 69ad5a6495bf24f9afd13a7a3ca19074f5d222ca 139
      fb664400597ce0918c57f5e755eab6d4f2afa725 e6fd2b5fc895fb648cad93b3507c3260a775a762 0000000000000000000000000000000000000000 0000000000000000000000000000000000000000 e6fd2b5fc895fb648cad93b3507c3260a775a762 149
  
      manifest
  
      thomas-test/5.rst
      b80de5d138758541c5f05265ad144ab9fa86d1db 0000000000000000000000000000000000000000 0000000000000000000000000000000000000000 0000000000000000000000000000000000000000 0000000000000000000000000000000000000000 20
  
      thomas-test/6.rst
      b80de5d138758541c5f05265ad144ab9fa86d1db 0000000000000000000000000000000000000000 0000000000000000000000000000000000000000 0000000000000000000000000000000000000000 0000000000000000000000000000000000000000 20
  pushkey -- {key: fb664400597ce0918c57f5e755eab6d4f2afa725, namespace: phases, new: 0, old: 1}
  pushkey -- {key: master, namespace: bookmarks, new: fb664400597ce0918c57f5e755eab6d4f2afa725, old: 4861b3248c40901dc09c91aae80750c2b802aa7e}




