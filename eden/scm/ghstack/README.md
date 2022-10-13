# sapling-ghstack

This is a fork of https://github.com/ezyang/ghstack that includes changes to
support Git-on-Hg.

## Building and Running the Code

Unlike upstream `ghstack`, this does not rely on `poetry` or any third-party
Python libraries (though it does require Python 3.8 or later whereas upstream
works with Python 3.6+). By avoiding third-party deps, the code can be run
directly via:

```
PYTHONPATH=path/to/ghstack python3 -m ghstack.__main__
```
