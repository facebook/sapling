# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# This is an example brew formula. It will need to be updated to point to an
# actual URL, with an actual sha256, license, and tests.
class Sapling < Formula
  desc "The Sapling source control client"
  homepage ""
  url "file:///Users/$USER/sapling.tar.gz"
  version "0.1"
  sha256 ""
  license ""

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
      system "make PREFIX=#{prefix} install-oss"
    end
  end

  test do
    # TODO
    system "true"
  end
end
