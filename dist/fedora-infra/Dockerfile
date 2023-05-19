FROM registry.fedoraproject.org/fedora:38

# build: system utilities and libraries
RUN dnf -y install g++ openssl-devel

# build: system Rust toolchain
RUN dnf -y install rust cargo

# build: project sources
ADD . /src
WORKDIR /src

# build: release binary
RUN cargo build --release && \
  mv /src/target/release/fcos-graph-builder /usr/local/bin/fcos-graph-builder && \
  mv /src/target/release/fcos-policy-engine /usr/local/bin/fcos-policy-engine

# build: cleanup
RUN cargo clean

# run: default config
WORKDIR /
