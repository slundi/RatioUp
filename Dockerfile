FROM rust:latest as builder

RUN apt-get update && apt-get install -y musl-tools musl-dev

#RUN rustc --version &&  rustup --version && cargo --version

WORKDIR /code

# Download crates-io index and fetch dependency code.
# This step avoids needing to spend time on every build downloading the index
# which can take a long time within the docker context. Docker will cache it.
#RUN USER=root cargo init
COPY Cargo.lock Cargo.lock
COPY Cargo.toml Cargo.toml
#RUN cargo fetch

# build dependencies, when my source code changes, this build can be cached, we don't need to compile dependency again.
COPY src src
RUN cargo clean && cargo build --release

# build with x86_64-unknown-linux-musl to make it run with alpine. $(uname -m)
#RUN cargo install --path . --target=$(uname -m)-unknown-linux-musl

# second stage.
FROM scratch
WORKDIR /data
ENV WEBROOT=/
# copy server binary from build stage
COPY --from=builder /code/target/release/RatioUp /app/RatioUp

LABEL author="Slundi"
LABEL url="https://github.com/slundi/RatioUp"
LABEL vcs-url="https://github.com/slundi/RatioUp"
# set user to non-root unless root is required for your app
USER 1001
EXPOSE 8070
ENTRYPOINT [ "/app/RatioUp", "--root", ${WEBROOT}]
