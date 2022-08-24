class Sapling < Formula
  desc "The Sapling source control client"
  homepage ""
  url "file:///Users/durham/sapling.tar.gz"
  version "0.1"
  sha256 ""
  license ""

  depends_on "cmake" => :build
  depends_on "openssl@1.1" => :build
  depends_on "python@3.8" => :build
  depends_on "rust" => :build

  def install
    ENV["DESTDIR"] = prefix
    ENV["OPENSSL_DIR"] = Formula["openssl@1.1"].opt_prefix
    ENV["PYTHON_SYS_EXECUTABLE"] = Formula["python@3.8"].opt_prefix/"bin/python3.8"

    cd "eden/scm" do
      system "make install-oss"
    end
  end

  test do
    # TODO
    system "true"
  end
end
