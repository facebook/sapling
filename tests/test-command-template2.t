  $ newrepo

Test ifgt function

  $ hg log -T '{ifgt(2, 1, "GT", "NOTGT")} {ifgt(2, 2, "GT", "NOTGT")} {ifgt(2, 3, "GT", "NOTGT")}\n' -r null
  GT NOTGT NOTGT

  $ hg log -T '{ifgt("2", "1", "GT", "NOTGT")} {ifgt("2", "2", "GT", "NOTGT")} {ifgt("2", 3, "GT", "NOTGT")}\n' -r null
  GT NOTGT NOTGT

