FROM ubuntu:22.04

RUN apt update

RUN DEBIAN_FRONTEND=noninteractive apt install -y \
        --allow-unauthenticated --allow-downgrades --allow-change-held-packages --no-install-recommends \
        sudo \
        build-essential \
        tzdata \
        ca-certificates \
        openssh-server \
        curl \
        wget \
        bison \
        iproute2 \
        iputils-ping dnsutils pciutils \
        iftop \
        unzip \
        cmake \
        git \
        lsb-release \
        numactl

RUN DEBIAN_FRONTEND=noninteractive apt install -y \
        --allow-unauthenticated --allow-downgrades --allow-change-held-packages --no-install-recommends \
        libclang-dev libnuma-dev librdmacm-dev libibverbs-dev protobuf-compiler

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
RUN echo 'source $HOME/.cargo/env' >> $HOME/.bashrc
ENV PATH="/root/.cargo/bin:${PATH}"
RUN cargo install cargo-make

RUN git clone https://github.com/phoenix-dataplane/phoenix.git --recursive

WORKDIR phoenix
