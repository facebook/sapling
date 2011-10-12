import doctest, tempfile, os, sys

if __name__ == "__main__":
    fd, name = tempfile.mkstemp(suffix='hg-tst')
    os.write(fd, sys.stdin.read())
    os.close(fd)
    failures, _ = doctest.testfile(name, module_relative=False)
    if failures:
        sys.exit(1)
