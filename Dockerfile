FROM rust:latest as builder

RUN rustup target add x86_64-unknown-linux-musl aarch64-unknown-linux-musl armv7-unknown-linux-musleabi

RUN apt-get update && apt-get install -y musl-tools musl-dev

#RUN rustc --version &&  rustup --version && cargo --version

WORKDIR /code

# Download crates-io index and fetch dependency code.
# This step avoids needing to spend time on every build downloading the index
# which can take a long time within the docker context. Docker will cache it.
#RUN USER=root cargo init
COPY ./ .

# build dependencies, when my source code changes, this build can be cached, we don't need to compile dependency again.
RUN cargo clean && cargo build --target x86_64-unknown-linux-musl --release


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
