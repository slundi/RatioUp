FROM rust:alpine as builder
RUN apk add --no-cache build-base

# Encourage some layer caching here rather then copying entire directory that includes docs to builder container ~CMN
WORKDIR /usr/src/rustscan
COPY Cargo.toml Cargo.lock ./
COPY src/ src/
RUN cargo install --path .

FROM alpine
LABEL author="Slundi"
RUN mkdir /app /config
WORKDIR /config
COPY --from=builder /usr/local/cargo/bin/RatioUp /app/RatioUp
ENTRYPOINT [ "/app/RatioUp" ]
