FROM rust:alpine as builder

WORKDIR /code

#RUN apk add --no-cache build-base
RUN apk add musl-dev

# Download crates-io index and fetch dependency code.
# This step avoids needing to spend time on every build downloading the index
# which can take a long time within the docker context. Docker will cache it.
RUN USER=root cargo init
COPY Cargo.toml Cargo.toml
RUN cargo fetch

# build dependencies, when my source code changes, this build can be cached, we don't need to compile dependency again.
COPY src src
RUN cargo build --release

# build with x86_64-unknown-linux-musl to make it run with alpine. $(uname -m)
#RUN cargo install --path . --target=$(uname -m)-unknown-linux-musl

# second stage.
FROM alpine
WORKDIR /app
# copy server binary from build stage
#COPY --from=builder /usr/local/cargo/bin/* /usr/local/bin
COPY --from=builder /code/target/release/RatioUp /app/RatioUp

LABEL author="Slundi"
RUN mkdir /app /config
WORKDIR /config
# set user to non-root unless root is required for your app
USER 1001
EXPOSE 7070
ENTRYPOINT [ "/app/RatioUp" ]
