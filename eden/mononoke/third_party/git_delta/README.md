`original_sources` folder contains a copy of git's internal delta library with all common deps it
needs, no patching was neccessary so it can likely be updated in the future by fetching newer versions.

bridge and src directories create ffi bindings allowing git delta generation from Rust.

Copy was made from: https://github.com/git/git/tree/3bd955d26919e149552f34aacf8a4e6368c26cec
