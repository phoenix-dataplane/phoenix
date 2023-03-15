FROM mcr.microsoft.com/devcontainers/rust:0-1-bullseye

RUN apt-get update && export DEBIAN_FRONTEND=noninteractive \
    && apt install -y libclang-dev libnuma-dev librdmacm-dev libibverbs-dev protobuf-compiler

RUN cargo install cargo-make