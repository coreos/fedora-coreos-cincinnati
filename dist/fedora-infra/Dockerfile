FROM fedora:31

# build: system utilities and libraries
RUN dnf -y install g++ openssl-devel

# build: system Rust toolchain
RUN dnf -y install rust cargo

# build: project sources
ADD . /src
WORKDIR /src

# build: release binary
RUN cargo build --release && \
  mv /src/target/release/dumnati /usr/local/bin/dumnati

# build: cleanup
RUN cargo clean

# run: default config
WORKDIR /
CMD [ "/usr/local/bin/dumnati" ]
