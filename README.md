# Mononoke

Mononoke is a next-generation server for the [Mercurial source control
system](https://www.mercurial-scm.org/), meant to scale up to accepting
thousands of commits every hour across millions of files. It is primarily
written in the [Rust programming language](https://www.rust-lang.org/en-US/).

## Caveat Emptor

Mononoke is still in early stages of development. We are making it available now because we plan to
start making references to it from our other open source projects such as
[Eden](https://github.com/facebookexperimental/eden).

**The version that we provide on GitHub does not build yet**.

This is because the code is exported verbatim from an internal repository at Facebook, and
not all of the scaffolding from our internal repository can be easily extracted. The key areas
where we need to shore things up are:

* Full support for a standard `cargo build`.
* Open source replacements for Facebook-internal services (blob store, logging etc).

The current goal is to get Mononoke working on Linux. Other Unix-like OSes may
be supported in the future

## Contributing

See the [CONTRIBUTING](CONTRIBUTING.md) file for how to help out.

## License
Mononoke is GNU General Public License licensed, as found in the
[LICENSE](LICENSE) file.
