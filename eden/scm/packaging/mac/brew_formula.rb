# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# This is an example brew formula. It will need to be updated to point to an
# actual URL, with an actual sha256, license, and tests.
class Sapling < Formula
  desc "The Sapling source control client"
  homepage ""
  license ""
  # These fields are intended to be populated by a Github action
  url "%URL%"
  version "%VERSION%"
  sha256 "%SHA256%"

  depends_on "python@3.8"
  depends_on "node"
  depends_on "cmake" => :build
  depends_on "openssl@1.1" => :build
  depends_on "rust" => :build
  depends_on "yarn" => :build

  def install
    ENV["OPENSSL_DIR"] = Formula["openssl@1.1"].opt_prefix
    ENV["PYTHON_SYS_EXECUTABLE"] = Formula["python@3.8"].opt_prefix/"bin/python3.8"
    ENV["PYTHON"] = Formula["python@3.8"].opt_prefix/"bin/python3.8"
    ENV["PYTHON3"] = Formula["python@3.8"].opt_prefix/"bin/python3.8"

    cd "eden/scm" do
      # Since above we make openssl a build-time dependency, we need to
      # statically link the OpenSSL library. In the openssl Rust crate, which
      # we use, this is done via setting the OPENSSL_STATIC environment variable
      #
      # The VERSION environment variable sets the version, and this is expected
      # to be filled by a Github action
      system "OPENSSL_STATIC=1 SAPLING_VERSION=%VERSION% " \
             "make PREFIX=#{prefix} install-oss"
    end
  end
end
