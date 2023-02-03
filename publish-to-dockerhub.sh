#!/bin/bash
echo "You need to be logged in, use `docker login`"

# rustup target add x86_64-unknown-linux-musl aarch64-unknown-linux-musl armv7-unknown-linux-musleabi armv7-unknown-linux-musleabihf
# mkdir /tmp/rustup
# cp -r static .env Dockerfile /tmp/rustup


docker pull messense/rust-musl-cross:aarch64-musl
docker pull messense/rust-musl-cross:armv7-musleabi
docker pull messense/rust-musl-cross:armv7-musleabihf
docker pull messense/rust-musl-cross:x86_64-musl
docker pull messense/rust-musl-cross:i686-musl

VERSION=$(grep -Po '\bversion\s*=\s*"\K.*?(?=")' Cargo.toml | head -n 1)
echo "Building RationUp $VERSION"

docker run --rm -it -v "$(pwd)":/home/rust/src messense/rust-musl-cross:x86_64-musl cargo build --release
#cargo build --release --target x86_64-unknown-linux-musl
# cp -f target/release/RatioUp ./RatioUp
docker buildx build --platform linux/x86_64 -t slundi/ratioup:latest -t slundi:$VERSION .

# cargo build --release --target aarch64-unknown-linux-musl
docker run --rm -it -v "$(pwd)":/home/rust/src messense/rust-musl-cross:aarch64-musl cargo build --release
# cp -f target/release/RatioUp ./RatioUp
docker buildx build --platform linux/aarch64 -t slundi/ratioup:latest -t slundi:$VERSION .

# cargo build --release --target armv7-unknown-linux-musleabi
docker run --rm -it -v "$(pwd)":/home/rust/src messense/rust-musl-cross:armv7-musleabi cargo build --release
# cp -f target/release/RatioUp ./RatioUp
docker buildx build --platform linux/armv7 -t slundi/ratioup:latest -t slundi:$VERSION .

# cargo build --release --target armv7-unknown-linux-musleabihf
docker run --rm -it -v "$(pwd)":/home/rust/src messense/rust-musl-cross:armv7-musleabihf cargo build --release
# cp -f target/release/RatioUp ./RatioUp
docker buildx build --platform linux/armv7hf -t slundi/ratioup:latest -t slundi:$VERSION .

# docker manifest create slundi/ratioup:latest slundi/ratioup:latest_x86_64 slundi/ratioup:latest_aarch64
# docker manifest push slundi/ratioup:latest

# docker buildx build --push --platform linux/arm/v6,linux/arm/v7,linux/arm64,linux/amd64 --tag slundi/ratioup:$VERSION --tag slundi/ratioup:latest .
docker buildx build --push --platform linux/arm/v6,linux/arm/v7,linux/arm64,linux/amd64 --tag slundi/ratioup:$VERSION --tag slundi/ratioup:latest .

# rm ./RustUp
