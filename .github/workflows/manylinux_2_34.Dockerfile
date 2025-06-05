# https://github.com/pypa/manylinux base image that work in many Linux distros
FROM quay.io/pypa/manylinux_2_34 AS base

# Paths used by the below scripts.
ENV PYTHON_SYS_EXECUTABLE=/opt/python/cp312-cp312/bin/python3.12
ENV PATH=/root/.nvm/versions/node/v22.16.0/bin:/opt/python/cp312-cp312/bin:/opt/node-v22.16.0-linux-x64/bin:/root/.cargo/bin:$PATH


# Python related build dependencies.
# - Used by setup.py
RUN /opt/python/cp312-cp312/bin/pip install setuptools
# - Extract libpython.a for static linking.
RUN ( cd /opt/_internal && tar -xf static-libs-for-embedding-only.tar.xz )


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

# Rust related build dependencies.
# - clang-devel: used by bindgen, used by zstd-sys
# - openssl-devel: used by curl-sys
RUN dnf install -y clang-devel openssl-devel


# Populate the Yarn offline mirror in a "fork".
FROM base AS populate-offline-cache
COPY . /tmp/repo
WORKDIR /tmp/repo
RUN yarn --cwd addons install --prefer-offline


# Get the yarn-offline-mirror. Discard changes (ex. node_modules/) in the working copy.
FROM base AS main
COPY --from=populate-offline-cache /root/npm-packages-offline-cache /root/npm-packages-offline-cache
