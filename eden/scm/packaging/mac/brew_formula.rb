# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# This is an example brew formula. It will need to be updated to point to an
# actual URL, with an actual sha256, license, and tests.
class Sapling < Formula
  desc "The Sapling source control client"
  homepage "https://sapling-scm.com"
  license "GPL-2.0-or-later"
  # These fields are intended to be populated by a Github action
  url "%URL%"
  version "%VERSION%"
  sha256 "%SHA256%"

  depends_on "python@3.11"
  depends_on "node"
  depends_on "openssl@1.1"
  depends_on "gh"
  depends_on "cmake" => :build
  depends_on "rustup-init" => :build
  depends_on "yarn" => :build

  def install
    # We use the openssl rust crate, which has its own mechanism for figuring
    # out where the OpenSSL installation is.
    # According to  https://docs.rs/openssl/latest/openssl/#manual , we can
    # force some specific location by setting the OPENSSL_DIR environment
    # variable. This is necessary since the installed OpenSSL library
    # might not match the architecture of the destination one.
    ENV["OPENSSL_DIR"] = "%TMPDIR%/openssl@1.1/1.1.1s"
    ENV["PYTHON_SYS_EXECUTABLE"] = Formula["python@3.11"].opt_prefix/"bin/python3.11"
    ENV["PYTHON"] = Formula["python@3.11"].opt_prefix/"bin/python3.11"
    ENV["PYTHON3"] = Formula["python@3.11"].opt_prefix/"bin/python3.11"
    ENV["SAPLING_VERSION"] = "%VERSION%"
    ENV["CFLAGS"] = "--target=%TARGET%"
    ENV["RUST_TARGET"] = "%TARGET%"
    # The line below is necessary, since otherwise homebrew somehow injects
    # -march=... into clang
    ENV["HOMEBREW_OPTFLAGS"] = ""

    cd "eden/scm" do
      system "rustup-init -y"
      system "source %CACHEDIR%/cargo_cache/env && rustup target add %TARGET%"
      system "source %CACHEDIR%/cargo_cache/env && "\
             "make PREFIX=#{prefix} install-oss"
    end
  end
end
