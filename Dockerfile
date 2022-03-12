FROM rust:alpine as builder

WORKDIR /app

#RUN apk add --no-cache build-base
RUN apk add musl-dev

# create a new empty project
RUN cargo init

COPY ./.cargo .cargo
COPY ./vendor vendor
COPY Cargo.toml Cargo.lock ./
# build dependencies, when my source code changes, this build can be cached, we don't need to compile dependency again.
RUN cargo build --release
# remove the dummy build.
RUN cargo clean -p RatioUp

# build with x86_64-unknown-linux-musl to make it run with alpine. $(uname -m)
RUN cargo install --path . --target=$(uname -m)-unknown-linux-musl

# second stage.
FROM alpine
COPY --from=builder /usr/local/cargo/bin/* /usr/local/bin

LABEL author="Slundi"
RUN mkdir /app /config
WORKDIR /config
COPY --from=builder /usr/local/cargo/bin/RatioUp /app/RatioUp
ENTRYPOINT [ "/app/RatioUp" ]
EXPOSE 7070
