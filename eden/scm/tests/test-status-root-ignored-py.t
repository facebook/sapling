#debugruntest-compatible
#require fsmonitor no-eden

  $ configure modernclient
  $ newclientrepo

Ensure that, when files in the root are ignored and there is an exclusion, that hg status returns the correct value

  $ cat > .gitignore << 'EOF'
  > /*
  > !/foobar
  > EOF
  $ hg status
  $ mkdir foobar
  $ touch root-file foobar/foo # adds files to root and to foobar
  $ hg status
  ? foobar/foo
  $ hg status # run it a second time to ensure that we didn't accidentally exclude the file
  ? foobar/foo
