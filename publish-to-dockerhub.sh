#!/bin/bash
echo "You need to be logged in, use `docker login`"
set -e
# rustup target add x86_64-unknown-linux-musl aarch64-unknown-linux-musl armv7-unknown-linux-musleabi armv7-unknown-linux-musleabihf
# mkdir /tmp/rustup
# cp -r static .env Dockerfile /tmp/rustup


docker pull messense/rust-musl-cross:aarch64-musl
docker pull messense/rust-musl-cross:armv7-musleabi
docker pull messense/rust-musl-cross:armv7-musleabihf
docker pull messense/rust-musl-cross:x86_64-musl
# docker pull messense/rust-musl-cross:i686-musl

TEMP_DIR=/tmp/RatioUp
SOURCE_DIR=$(pwd)
VERSION=$(grep -Po '\bversion\s*=\s*"\K.*?(?=")' Cargo.toml | head -n 1)

mkdir -p /tmp/RatioUp/{x86_64,aarch64,armv7,armv7hf}
cp -r Dockerfile Docker.env static $TEMP_DIR/x86_64/
cp -r Dockerfile Docker.env static $TEMP_DIR/aarch64/
cp -r Dockerfile Docker.env static $TEMP_DIR/armv7/
cp -r Dockerfile Docker.env static $TEMP_DIR/armv7hf/

echo "Building RatioUp $VERSION"

# build all binaries
#docker run --rm -it -v "$SOURCE_DIR":/home/rust/src messense/rust-musl-cross:x86_64-musl cargo build --release
#docker run --rm -it -v "$SOURCE_DIR":/home/rust/src messense/rust-musl-cross:aarch64-musl cargo build --release
#docker run --rm -it -v "$SOURCE_DIR":/home/rust/src messense/rust-musl-cross:armv7-musleabi cargo build --release
#docker run --rm -it -v "$SOURCE_DIR":/home/rust/src messense/rust-musl-cross:armv7-musleabihf cargo build --release

echo "Creating images"

cd $TEMP_DIR/x86_64
cp -f $SOURCE_DIR/target/x86_64-unknown-linux-musl/release/RatioUp $TEMP_DIR/x86_64/RatioUp
# docker buildx build --platform linux/x86_64 -t slundi/ratioup:latest_x86_64 .
docker buildx build --push --platform linux/x86_64 --tag slundi/ratioup:latest_x86_64 .
# docker buildx build --push --platform linux/x86_64 --tag slundi/ratioup:latest_x86_64 --tag slundi/ratioup:latest --tag slundi/ratioup:$VERSION .

# cd $TEMP_DIR/aarch64/
# cp -f $SOURCE_DIR/target/aarch64-unknown-linux-musl/release/RatioUp $TEMP_DIR/aarch64/RatioUp
# docker buildx build --push --platform linux/aarch64 --tag slundi/ratioup:latest_aarch64 .

# cd $TEMP_DIR/armv7/
# cp -f $SOURCE_DIR/target/armv7-unknown-linux-musleabi/release/RatioUp $TEMP_DIR/armv7/RatioUp
# docker buildx build --push --platform linux/armv7 --tag slundi/ratioup:latest_armv7 .

# cd $TEMP_DIR/armv7hf/
# cp -f $SOURCE_DIR/target/armv7-unknown-linux-musleabihf/release/RatioUp $TEMP_DIR/armv7hf/RatioUp
# docker buildx build --push --platform linux/armv7hf --tag slundi/ratioup:latest_armv7hf .

# docker manifest create slundi/ratioup:latest slundi/ratioup:latest_x86_64 slundi/ratioup:latest_aarch64
# docker manifest push slundi/ratioup:latest

# docker buildx build --push --platform linux/armv7,linux/armv7hf,linux/aarch64,linux/x86_64 --tag slundi/ratioup:$VERSION --tag slundi/ratioup:latest .
# docker buildx build --push --platform linux/amd64 --tag slundi/ratioup:$VERSION --tag slundi/ratioup:latest .

docker manifest rm slundi/ratioup:latest slundi/ratioup:$VERSION slundi/ratioup:latest_x86_64 slundi/ratioup:latest_aarch64 slundi/ratioup:latest_armv7 slundi/ratioup:latest_armv7hf
docker manifest create slundi/ratioup:latest slundi/ratioup:latest_x86_64 slundi/ratioup:latest_aarch64 slundi/ratioup:latest_armv7 slundi/ratioup:latest_armv7hf
docker manifest create slundi/ratioup:$VERSION slundi/ratioup:latest_x86_64 slundi/ratioup:latest_aarch64 slundi/ratioup:latest_armv7 slundi/ratioup:latest_armv7hf
docker manifest push slundi/ratioup:latest
# docker manifest push slundi/ratioup:$VERSION

rm -r $TEMP_DIR
