# Sapling

Sapling is a fast, easy to use, distributed revision control tool for software
developers.


# Basic install

```
$ make install-oss
$ sl debuginstall # sanity-check setup
$ sl --help       # see help
```


Running without installing:

```
$ make oss        # build for inplace usage
$ ./sl --version  # should show the latest version
```

See <https://sapling-scm.com/> for detailed installation instructions,
platform-specific notes, and Sapling user information.

# Thrift enabled getdeps CLI build for use with Mononoke or EdenFS

Mononoke and EdenFS need the thrift enabled sapling CLI built via getdeps. Check github actions to see current OS version the Sapling CLI Getdeps CI runs with.

This build also provides a way to run the sapling .t tests in github CI and locally.

When building locally you don't need to separately build all the dependencies like the github CI does, command line steps are:

make sure required system packages are installed:
`./build/fbcode_builder/getdeps.py install-system-deps --recursive sapling`

build sapling (and any dependencies it requires):
`./build/fbcode_builder/getdeps.py build --allow-system-packages --src-dir=. sapling`

you can find the built binaries via:
`./build/fbcode_builder/getdeps.py show-inst-dir --allow-system-packages --src-dir=. sapling`

run the tests (you can use --num-jobs=N to adjust concurrency):
`./build/fbcode_builder/getdeps.py test --allow-system-packages --src-dir=. --num-jobs=64 sapling`

to iterate on one test run with --retry 0 --filter:
`./build/fbcode_builder/getdeps.py test --allow-system-packages --src-dir=. sapling --retry 0 --filter test-check-execute.t`

to run multiple tests with --filter separate with spaces:
`./build/fbcode_builder/getdeps.py test --allow-system-packages --src-dir=. sapling --retry 0 --filter "test-include-fail.t test-matcher-lots-of-globs.t"`

The getdeps build doesn't currently build/test ISL or other node integration.
