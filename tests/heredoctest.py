import doctest, tempfile, os, sys

if __name__ == "__main__":
    if 'TERM' in os.environ:
        del os.environ['TERM']

    fd, name = tempfile.mkstemp(suffix='hg-tst')

    try:
        os.write(fd, sys.stdin.read())
        os.close(fd)
        failures, _ = doctest.testfile(name, module_relative=False)
        if failures:
            sys.exit(1)
    finally:
        os.remove(name)
