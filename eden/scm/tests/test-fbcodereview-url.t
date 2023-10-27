#debugruntest-compatible

  $ newrepo

  $ enable fbcodereview
  $ cat >> .hg/hgrc << 'EOF'
  > [fbscmquery]
  > reponame=foo
  > [fbcodereview]
  > code-browser-url=https://example.com/%(repo_name)s/%(node_hex)s/%(path)s
  > EOF

  $ drawdag << EOS
  > A
  > EOS

  $ hg url -r 'desc(A)' A 'b/d#@!/%s /拉链.zip'
  https://example.com/foo/426bada5c67598ca65036d57d9e4b64b0c1ce7a0/A
  https://example.com/foo/426bada5c67598ca65036d57d9e4b64b0c1ce7a0/b/d%23%40%21/%25s%20/%E6%8B%89%E9%93%BE.zip
