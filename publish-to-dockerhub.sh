#!/bin/bash
echo "You need to be logged in, use `docker login`"
set -e
rustup target add x86_64-unknown-linux-musl aarch64-unknown-linux-musl armv7-unknown-linux-musleabi armv7-unknown-linux-musleabihf x86_64-apple-darwin aarch64-apple-darwin
# mkdir /tmp/rustup
# cp -r static .env Dockerfile /tmp/rustup


# docker pull messense/rust-musl-cross:aarch64-musl
# docker pull messense/rust-musl-cross:armv7-musleabi
# docker pull messense/rust-musl-cross:x86_64-musl
# docker pull messense/rust-musl-cross:i686-musl

VERSION=$(grep -Po '\bversion\s*=\s*"\K.*?(?=")' Cargo.toml | head -n 1)
echo "Building RationUp $VERSION"

if [ ! -d builds ]; then
  mkdir builds
fi

cargo build -r --target x86_64-unknown-linux-musl
mv target/x86_64-unknown-linux-musl/release/RatioUp builds/RatioUp_x86_64
rm -r target/x86_64-unknown-linux-musl

cargo build -r --target aarch64-unknown-linux-musl
mv target/aarch64-unknown-linux-musl/release/RatioUp builds/RatioUp_aarch64
rm -r target/aarch64-unknown-linux-musl

cargo build -r --target armv7-unknown-linux-musleab
mv target/armv7-unknown-linux-musleab/release/RatioUp builds/RatioUp_armv7musleab
rm -r target/armv7-unknown-linux-musleab

cargo build -r --target armv7-unknown-linux-musleabihf
mv target/armv7-unknown-linux-musleabihf/release/RatioUp builds/RatioUp_armv7musleabihf
rm -r target/armv7-unknown-linux-musleabihf

cargo build -r --target x86_64-apple-darwin
mv target/x86_64-apple-darwin/release/RatioUp builds/RatioUp_x86_64-apple
rm -r target/x86_64-apple-darwin

cargo build -r --target aarch64-apple-darwin
mv target/aarch64-apple-darwin/release/RatioUp builds/RatioUp_aarch64-apple
rm -r target/aarch64-apple-darwin

# docker run --rm -it -v "$(pwd)":/home/rust/src messense/rust-musl-cross:x86_64-musl cargo build --release
# cp -f target/x86_64-unknown-linux-musl/release/RatioUp ./RatioUp
# docker buildx build --platform linux/x86_64 -t slundi/ratioup:latest -t slundi:$VERSION .

# docker run --rm -it -v "$(pwd)":/home/rust/src messense/rust-musl-cross:aarch64-musl cargo build --release
# cp -f target/aarch64-unknown-linux-musl/release/RatioUp ./RatioUp
# docker buildx build --platform linux/aarch64 -t slundi/ratioup:latest -t slundi:$VERSION .

# docker run --rm -it -v "$(pwd)":/home/rust/src messense/rust-musl-cross:armv7-musleabi cargo build --release
# cp -f target/armv7-unknown-linux-musleabi/release/RatioUp ./RatioUp
# docker buildx build --platform linux/armv7 -t slundi/ratioup:latest -t slundi:$VERSION .

# docker run --rm -it -v "$(pwd)":/home/rust/src messense/rust-musl-cross:armv7-musleabihf cargo build --release
# cp -f target/armv7-unknown-linux-musleabihf/release/RatioUp ./RatioUp
# docker buildx build --platform linux/armv7hf -t slundi/ratioup:latest -t slundi:$VERSION .

# docker manifest create slundi/ratioup:latest slundi/ratioup:latest_x86_64 slundi/ratioup:latest_aarch64
# docker manifest push slundi/ratioup:latest

# docker buildx build --push --platform linux/armv7,linux/armv7hf,linux/aarch64,linux/x86_64 --tag slundi/ratioup:$VERSION --tag slundi/ratioup:latest .
# docker buildx build --push --platform linux/amd64 --tag slundi/ratioup:$VERSION --tag slundi/ratioup:latest .

# for arch in x86_64 aarch64 armv7 armv7hf; do
#     docker build -f Dockerfile.${arch} -t slundi/ratioup:${arch}-latest -t slundi/ratioup:${arch}-$VERSION  .
#     echo slundi/ratioup:${arch}-latest -t slundi/ratioup:${arch}-$VERSION
#     docker push slundi/ratioup:${arch}-latest
#     docker push slundi/ratioup:${arch}-$VERSION
# done

# docker buildx build --platform linux/x86_64  -f Dockerfile.x86_64  -t slundi/ratioup:x86_64-latest  -t slundi/ratioup:x86_64-$VERSION .
# docker buildx build --platform linux/aarch64 -f Dockerfile.aarch64 -t slundi/ratioup:aarch64-latest -t slundi/ratioup:aarch64-$VERSION .
# docker buildx build --platform linux/armv7   -f Dockerfile.armv7   -t slundi/ratioup:armv7-latest   -t slundi/ratioup:armv7-$VERSION .

# docker push slundi/ratioup:x86_64-latest
# docker push slundi/ratioup:x86_64-$VERSION
# docker push slundi/ratioup:aarch64-latest
# docker push slundi/ratioup:aarch64-$VERSION

# docker manifest create slundi/ratioup:latest slundi/ratioup:x86_64-latest slundi/ratioup:aarch64-latest slundi/ratioup:armv7-latest slundi/ratioup:armv7hf-latest
docker manifest create slundi/ratioup:latest slundi/ratioup:x86_64-latest slundi/ratioup:aarch64-latest
docker manifest annotate slundi/ratioup:latest slundi/ratioup:x86_64-latest  --os linux --arch x86_64
# docker manifest annotate slundi/ratioup:latest slundi/ratioup:armv7-latest --os linux --arch arm7
# docker manifest annotate slundi/ratioup:latest slundi/ratioup:armv7hf-latest --os linux --arch arm7hf
docker manifest annotate slundi/ratioup:latest slundi/ratioup:aarch64-latest --os linux --arch aarch64 --variant armv8
docker manifest push slundi/ratioup:latest

# cp target/x86_64-unknown-linux-musl/release/XXX XXX
# docker buildx build --platform linux/amd64 -t slundi/ratioup:latest_amd64 --push .
# cp target/aarch64-unknown-linux-musl/release/XXX XXX
# docker buildx build --platform linux/arm64 -t slundi/ratioup:latest_arm64 --push .
# docker manifest create slundi/ratioup:latest slundi/ratioup:latest_amd64 slundi/ratioup:latest_arm64
# docker manifest push slundi/ratioup:latest
