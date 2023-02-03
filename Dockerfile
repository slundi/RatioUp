FROM --platform=linux/amd64 messense/rust-musl-cross:x86_64-musl as builder

# RUN rustc --version &&  rustup --version && cargo --version

# WORKDIR /code

# COPY Cargo.toml Cargo.toml
# RUN mkdir src/
# RUN echo "fn main() {println!(\"if you see this, the build broke\")}" > src/main.rs

# RUN cargo build --release > /log
# RUN rm -f target/release/deps/RatioUp-*

# # Download crates-io index and fetch dependency code.
# # This step avoids needing to spend time on every build downloading the index
# # which can take a long time within the docker context. Docker will cache it.
# COPY ./ /code

# build dependencies, when my source code changes, this build can be cached, we don't need to compile dependency again.
# RUN cargo clean && cargo build --release

# second stage.
FROM scratch
WORKDIR /app
ENV WEBROOT=/
# copy server binary from build stage
#COPY --from=builder /code/target/release/RatioUp /app/RatioUp
COPY static target/release/RatioUp ./

LABEL author="Slundi"
LABEL url="https://github.com/slundi/RatioUp"
LABEL vcs-url="https://github.com/slundi/RatioUp"
# set user to non-root unless root is required for your app
USER 1001
EXPOSE 8070
ENTRYPOINT [ "/app/RatioUp", "--root", ${WEBROOT}]
