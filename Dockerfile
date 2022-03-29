FROM rust:latest as builder

ARG TARGETPLATFORM

RUN apk add --no-cache musl-tools musl-dev

#RUN rustc --version &&  rustup --version && cargo --version

WORKDIR /code

# Download crates-io index and fetch dependency code.
# This step avoids needing to spend time on every build downloading the index
# which can take a long time within the docker context. Docker will cache it.
#RUN USER=root cargo init
COPY ./ /code

# build dependencies, when my source code changes, this build can be cached, we don't need to compile dependency again.
#RUN cargo clean && cargo build --release
RUN case $TARGETPLATFORM in\
      linux/amd64)  rust_target="x86_64-unknown-linux-musl";\
                    tini_static_arch="amd64";;\
      linux/arm64)  rust_target="aarch64-unknown-linux-musl";\
                    tini_static_arch="arm64";;\
      linux/arm/v7) rust_target="armv7-unknown-linux-musleabihf";\
                    tini_static_arch="armel";;\
      linux/arm/v6) rust_target="arm-unknown-linux-musleabi";\
                    tini_static_arch="armel";;\
      *)            exit 1;;\
    esac &&\
    cargo build --target ${rust_target} --release


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
