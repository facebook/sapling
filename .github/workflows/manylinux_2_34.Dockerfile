# https://github.com/pypa/manylinux base image that work in many Linux distros
FROM quay.io/pypa/manylinux_2_34 AS base

# Paths used by the below scripts.
ENV PYTHON_SYS_EXECUTABLE=/opt/python/cp312-cp312/bin/python3.12
ENV PATH=/root/.nvm/versions/node/v22.16.0/bin:/opt/python/cp312-cp312/bin:/opt/node-v22.16.0-linux-x64/bin:/root/.cargo/bin:$PATH


# Build dependencies.
# - clang-devel: used by bindgen, used by zstd-sys
# - perl: openssl build dependency (static openssl, and openssl-src Rust crate)
RUN dnf install -y clang-devel perl


# Rebuild OpenSSL as a static library so CPython's _ssl.so does not depend
# on a specific system libssl.so.3 / libcrypto.so.3 version at runtime.
# The stock manylinux image builds OpenSSL as shared libs, which means the
# _ssl.so extension carries versioned symbol requirements (e.g. OPENSSL_3.3.0)
# that most distros cannot satisfy.
RUN <<'SSLEOF'
set -ex

# Remove any stock shared OpenSSL so build-cpython.sh picks up ours.
rm -rf /opt/_internal/openssl-*

# Build OpenSSL as static-only with -fPIC.
cd /tmp
OPENSSL_VERSION=3.5.5
curl -LO "https://github.com/openssl/openssl/releases/download/openssl-${OPENSSL_VERSION}/openssl-${OPENSSL_VERSION}.tar.gz"
tar xf "openssl-${OPENSSL_VERSION}.tar.gz"
cd "openssl-${OPENSSL_VERSION}"
./config --prefix=/opt/_internal/openssl-3.5 --libdir=lib -fPIC \
  no-shared no-module no-tests no-apps no-docs \
  no-comp no-zlib no-legacy no-engine \
  no-afalgeng no-capieng no-padlockeng \
  no-ssl3 no-md2 no-rc5 no-weak-ssl-ciphers \
  no-camellia no-idea no-seed no-quic no-ui-console \
  no-aria no-bf no-cast no-des no-rc2 no-rc4 \
  no-md4 no-mdc2 no-rmd160 no-whirlpool no-siphash \
  no-sm2 no-sm3 no-sm4 no-gost no-siv no-ocb \
  no-cms no-cmp no-ct no-ocsp no-ts no-srp no-psk \
  no-dtls no-sctp no-srtp no-dgram \
  no-argon2 no-scrypt no-ec2m no-dso
make -j2
make install_sw
cd ..
rm -rf "openssl-${OPENSSL_VERSION}" "openssl-${OPENSSL_VERSION}.tar.gz"
SSLEOF


# Rebuild CPython 3.12 with -fPIC for static libpython.a linking.
# The stock manylinux libpython.a is built without -fPIC, which is annoying
# to work with rust (e.g. D75250624, and there is no easy/clean way to set
# rustc flags for build.rs). Rebuilding with -fPIC avoids that.
# Upstream issue: https://github.com/pypa/manylinux/pull/1258 it was not
# merged because of conerns about `-fPIC` slowing things now.
RUN <<'PYEOF'
python3 -c "
import re, pathlib
def patch(path, replacements):
    text = pathlib.Path(path).read_text()
    for old, new in replacements:
        assert old in text, f'{old!r} not found in {path}'
        text = text.replace(old, new)
    pathlib.Path(path).write_text(text)

# Add -fPIC to compiler flags
for var in ['MANYLINUX_CFLAGS', 'MANYLINUX_CXXFLAGS']:
    patch('/opt/_internal/build_scripts/build_utils.sh', [
        (f'{var}=\"-g', f'{var}=\"-fPIC -g'),
    ])

# Skip cosign signature verification (cosign not installed in base image)
patch('/opt/_internal/build_scripts/build-cpython.sh', [
    ('fetch_source \"Python-\${CPYTHON_VERSION}.tar.xz.sigstore\"', ': # fetch_source sigstore'),
    ('cosign  verify-blob', ': # cosign  verify-blob'),
    (r'make > /dev/null', r'make -j2 > /dev/null'),
])
"
PYEOF

# $1, $2 are for signing, they are not used after the above patches.
RUN /opt/_internal/build_scripts/build-cpython.sh x x \
    $($PYTHON_SYS_EXECUTABLE -c "import sys; print('.'.join(map(str, sys.version_info[:3])))")

# Python related build dependencies.
# - Used by setup.py
RUN /opt/python/cp312-cp312/bin/pip install setuptools


# Node.js interpreter.
# - Relatively up-to-date node.js. The dnf nodejs is 4+yrs older.
RUN curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.40.3/install.sh | bash
RUN source ~/.nvm/nvm.sh && nvm install 22.16.0

# Node.js global build dependencies.
# - yarn is used by ISL build.
RUN npm install -g yarn
RUN yarn config set yarn-offline-mirror "/root/npm-packages-offline-cache"


# Rust compiler.
# - Latest stable Rust from rustup. The dnf Rust is 2-month+ older.
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable --profile minimal


# Populate the Yarn offline mirror in a "fork".
FROM base AS populate-offline-cache
COPY . /tmp/repo
WORKDIR /tmp/repo
RUN yarn --cwd addons install --prefer-offline


# Get the yarn-offline-mirror. Discard changes (ex. node_modules/) in the working copy.
FROM base AS main
COPY --from=populate-offline-cache /root/npm-packages-offline-cache /root/npm-packages-offline-cache
