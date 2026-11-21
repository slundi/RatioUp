# --- Variables ---
binary_name := "RatioUp"

# Codeberg release config
codeberg_owner := "slundi"
codeberg_repo := "RatioUp"
codeberg_api := "https://codeberg.org/api/v1"

# GitHub release config (publishing needs `gh auth login`)
github_repo := "slundi/RatioUp"

# Remotes a release tag is pushed to
release_remotes := "origin Github"

# Targets cross-built locally via `cargo zigbuild` for releases (zig is the C
# compiler + linker for every target, no containers needed). Each one must also
# be listed in rust-toolchain.toml. Darwin targets can't be cross-built from
# Linux here: run the manual GitHub release workflow for those.
release_targets := "x86_64-unknown-linux-musl aarch64-unknown-linux-musl i686-unknown-linux-musl armv7-unknown-linux-musleabihf arm-unknown-linux-musleabihf x86_64-pc-windows-gnu"

# Linux targets also packaged as .deb. ARMv6 (arm-unknown-linux-musleabihf) is
# left out: its .deb would clash with the armv7 one (both are Debian `armhf`).
deb_targets := "x86_64-unknown-linux-musl aarch64-unknown-linux-musl i686-unknown-linux-musl armv7-unknown-linux-musleabihf"

# --- Development ---

# Build the project in debug mode
build:
    cargo build

# Run the project
run *args:
    cargo run -- {{args}}

# Watch for changes and run (requires cargo-watch)
watch:
    cargo watch -x run

# --- Quality Control ---

# Run all tests
test:
    cargo nextest run

# Run a full health check (Vulnerabilities, Unused Deps, Licenses)
health-check:
    cargo audit
    cargo machete
    cargo deny check

# Run clippy with strict warnings
lint:
    cargo clippy --all-targets --all-features -- -D warnings

# Format all code
fmt:
    cargo fmt --all

# Run all pre-commit hooks manually on all files
check:
    prek run --all-files

# Force update hooks
update-hooks:
    prek autoupdate

# Run gitleaks to scan for secrets
scan-secrets:
    gitleaks detect --verbose --redact

# Check that overall line coverage meets the 80% threshold
# BASE is accepted for interface compatibility with CI (e.g.: just coverage-check master)
coverage-check BASE="master":
    cargo llvm-cov --lcov --output-path lcov.info
    cargo llvm-cov report --fail-under-lines 80

# --- Cleanup ---

# Clean build artifacts
clean:
    cargo clean

# --- CI ---

# Run the complete CI pipeline (use this when already inside `nix develop`)
ci-all: fmt lint test health-check scan-secrets

# Run the complete CI pipeline via Nix devshell — no docker/podman needed
# Equivalent to what Woodpecker and Forgejo Actions run in containers
ci-local:
    nix develop --command just fmt
    nix develop --command just lint
    nix develop --command just test
    nix develop --command just health-check
    nix develop --command just scan-secrets
    @echo "All CI checks passed!"

# Run a quick subset (format + lint + test) — fast feedback loop
ci: fmt lint test
    @echo "Quick checks passed!"

# --- Changelog ---

# Generate or update CHANGELOG.md from conventional commits
changelog:
    git cliff -o CHANGELOG.md
    @echo "CHANGELOG.md updated — commit it before tagging"

# Preview changelog entries for commits not yet in a release
changelog-unreleased:
    git cliff --unreleased

# --- Release ---

# Push an annotated tag without building. Use this once CI handles release uploads.
# Usage: just tag v1.2.3
tag VERSION:
    git tag -a {{VERSION}} -m "Release {{VERSION}}"
    for remote in {{release_remotes}}; do git push "$remote" {{VERSION}}; done
    @echo "Pushed tag {{VERSION}}"

# Build a release binary for the current platform
build-release:
    cargo build --release

# Build a release binary for a specific cross-compilation target
# Requires `cargo-zigbuild` + `zig` (included in flake.nix devShell)
# Usage: just build-release-cross aarch64-unknown-linux-musl
build-release-cross TARGET:
    cargo zigbuild --release --target {{TARGET}}

# Assert a musl release binary is fully static (no ELF interpreter, no NEEDED
# libraries) and actually executes. Guards against accidentally reintroducing
# a runtime dependency (e.g. enabling zstd's pkg-config feature). Stronger and
# faster than any container run: if it's static it runs on any Linux.
# Usage: just verify-static x86_64-unknown-linux-musl
verify-static TARGET:
    #!/usr/bin/env bash
    set -euo pipefail
    bin="target/{{TARGET}}/release/{{binary_name}}"
    if readelf -l "$bin" | grep -q INTERP || readelf -d "$bin" 2>/dev/null | grep -q NEEDED; then
        echo "error: $bin is dynamically linked — expected a fully static musl build" >&2
        readelf -d "$bin" 2>/dev/null | grep NEEDED || true
        exit 1
    fi
    # Only execute binaries the host CPU can run (x86_64 also runs i686).
    arch="$(echo {{TARGET}} | cut -d- -f1)"
    host="$(uname -m)"
    if [[ "$arch" == "$host" || ( "$arch" == i686 && "$host" == x86_64 ) ]]; then
        "$bin" --version >/dev/null
        echo "  ✓ {{TARGET}} is fully static and runs"
    else
        echo "  ✓ {{TARGET}} is fully static (foreign arch, not executed)"
    fi

# Build and package a single release artifact into dist/
# Usage: just package v1.2.3 x86_64-unknown-linux-musl
package VERSION TARGET:
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p dist
    echo "Building {{binary_name}} {{VERSION}} for {{TARGET}}..."
    cargo zigbuild --release --target {{TARGET}}
    if [[ "{{TARGET}}" == *-linux-musl* ]]; then
        just verify-static {{TARGET}}
    fi
    stage="dist/{{binary_name}}-{{VERSION}}-{{TARGET}}"
    rm -rf "$stage"; mkdir -p "$stage"
    cp README.md LICENSE CHANGELOG.md "$stage/"
    if [[ "{{TARGET}}" == *windows* ]]; then
        cp "target/{{TARGET}}/release/{{binary_name}}.exe" "$stage/"
        (cd dist && zip -qr "{{binary_name}}-{{VERSION}}-{{TARGET}}.zip" "$(basename "$stage")")
    else
        cp "target/{{TARGET}}/release/{{binary_name}}" "$stage/"
        tar -C dist -czf "dist/{{binary_name}}-{{VERSION}}-{{TARGET}}.tar.gz" "$(basename "$stage")"
    fi
    rm -rf "$stage"
    echo "  packaged $(ls dist/{{binary_name}}-{{VERSION}}-{{TARGET}}.*)"

# Build a static MUSL binary and package it into a .deb (requires cargo-deb).
# Cross-builds with `cargo zigbuild`, so any target in `deb_targets` works from
# an amd64 host without Docker. Run inside `nix develop`.
# cargo-deb reads package metadata from Cargo.toml and maps the Rust target to
# the Debian arch (amd64, arm64, i386, armhf). The .deb lands in dist/.
# Usage: just deb [TARGET]
deb TARGET="x86_64-unknown-linux-musl":
    #!/usr/bin/env bash
    set -euo pipefail
    target="{{TARGET}}"
    mkdir -p dist
    cargo zigbuild --release --target "$target"
    just verify-static "$target"
    # --no-build: package the binary we just built (avoid a redundant rebuild).
    # --no-strip: keep symbols for addr2line on crash reports (see release profile).
    # cargo-deb prints the path of the generated package on stdout.
    deb="$(cargo deb --no-build --no-strip --target "$target" --output dist/)"
    echo "  packaged $deb"

nix-deb:
    nix develop --command just deb

# Build all locally-supported release targets + .debs and write SHA256SUMS
# Usage: just package-all v1.2.3
package-all VERSION:
    #!/usr/bin/env bash
    set -euo pipefail
    ver="{{VERSION}}"; ver="${ver#v}"
    rm -f dist/{{binary_name}}-{{VERSION}}-* dist/{{binary_name}}_"${ver}"-*.deb
    for target in {{release_targets}}; do
        just package {{VERSION}} "$target"
    done
    for target in {{deb_targets}}; do
        just deb "$target"
    done
    (cd dist && sha256sum {{binary_name}}-{{VERSION}}-*.tar.gz {{binary_name}}-{{VERSION}}-*.zip \
        {{binary_name}}_"${ver}"-*.deb > {{binary_name}}-{{VERSION}}-SHA256SUMS)
    echo "Artifacts:"
    just _assets {{VERSION}}

# Full release: validate, build all targets, tag + push to every remote, then
# create the Codeberg and GitHub releases with the same assets.
# Requires CODEBERG_TOKEN env var with write:repository scope
#   (Codeberg → Settings → Applications → Generate New Token)
# and an authenticated GitHub CLI (`gh auth login`).
# macOS binaries are not built locally: afterwards, run the GitHub "Release"
# workflow by hand if you want them on the GitHub release.
# Usage: just release v1.2.3
release VERSION: (_release-preflight VERSION) (package-all VERSION)
    @if command -v docker >/dev/null; then just docker-smoke {{VERSION}}; else echo "docker not found, skipping docker-smoke"; fi
    just _release-tag-push {{VERSION}}
    just _release-codeberg {{VERSION}}
    just _release-github {{VERSION}}
    @echo "✓ Release {{VERSION}} published:"
    @echo "  https://codeberg.org/{{codeberg_owner}}/{{codeberg_repo}}/releases/tag/{{VERSION}}"
    @echo "  https://github.com/{{github_repo}}/releases/tag/{{VERSION}}"

# --- Internal release helpers (prefixed _ to keep `just --list` clean) ---

# List the release assets for VERSION in dist/ (archives, .debs, SHA256SUMS)
_assets VERSION:
    #!/usr/bin/env bash
    set -euo pipefail
    shopt -s nullglob
    ver="{{VERSION}}"; ver="${ver#v}"
    printf '%s\n' dist/{{binary_name}}-{{VERSION}}-* dist/{{binary_name}}_"${ver}"-*.deb

_release-preflight VERSION:
    #!/usr/bin/env bash
    set -euo pipefail
    for tool in cargo-zigbuild zig cargo-deb gh jq zip git-cliff; do
        if ! command -v "$tool" >/dev/null; then
            echo "error: $tool not found — run inside \`nix develop\`" >&2
            exit 1
        fi
    done
    if [[ -z "${CODEBERG_TOKEN:-}" ]]; then
        echo "error: CODEBERG_TOKEN not set" >&2
        echo "  Create one at https://codeberg.org/user/settings/applications (scope: write:repository)" >&2
        exit 1
    fi
    if ! gh auth status --hostname github.com >/dev/null 2>&1; then
        echo "error: GitHub CLI is not authenticated — run \`gh auth login\`" >&2
        exit 1
    fi
    if git rev-parse -q --verify "refs/tags/{{VERSION}}" >/dev/null; then
        echo "error: tag {{VERSION}} already exists" >&2
        exit 1
    fi
    if ! git diff-index --quiet HEAD --; then
        echo "error: working tree has uncommitted changes" >&2
        exit 1
    fi
    expected="$(echo {{VERSION}} | sed 's/^v//')"
    actual="$(grep -m1 '^version = ' Cargo.toml | sed -E 's/.*"(.*)".*/\1/')"
    if [[ "$expected" != "$actual" ]]; then
        echo "error: VERSION ($expected) does not match Cargo.toml version ($actual)" >&2
        echo "  Update Cargo.toml + Cargo.lock, commit, then retry." >&2
        exit 1
    fi

# --atomic: the current branch and the tag land together on each remote, so a
# tag never points at a commit the remote branch doesn't have.
_release-tag-push VERSION:
    git tag -a {{VERSION}} -m "Release {{VERSION}}"
    for remote in {{release_remotes}}; do git push --atomic "$remote" HEAD {{VERSION}}; done

# Stage Linux release binaries next to the Dockerfile so `docker build` works.
# Pulls from local dist/ if `just package-all VERSION` has been run; otherwise
# fetches the tarballs from the Codeberg release for VERSION.
# Usage: just docker-context v0.2.0
docker-context VERSION:
    #!/usr/bin/env bash
    set -euo pipefail
    for pair in "amd64:x86_64-unknown-linux-musl" "arm64:aarch64-unknown-linux-musl"; do
        docker_arch="${pair%%:*}"
        rust_target="${pair##*:}"
        archive="dist/{{binary_name}}-{{VERSION}}-${rust_target}.tar.gz"
        if [[ ! -f "$archive" ]]; then
            echo "fetching ${archive} from Codeberg..."
            mkdir -p dist
            curl -fsSL "https://codeberg.org/{{codeberg_owner}}/{{codeberg_repo}}/releases/download/{{VERSION}}/$(basename "$archive")" \
                -o "$archive"
        fi
        rm -rf "dist/extract-${docker_arch}"
        mkdir -p "dist/extract-${docker_arch}"
        tar -xzf "$archive" -C "dist/extract-${docker_arch}"
        cp "dist/extract-${docker_arch}/{{binary_name}}-{{VERSION}}-${rust_target}/{{binary_name}}" \
            "{{binary_name}}-linux-${docker_arch}"
        rm -rf "dist/extract-${docker_arch}"
    done
    echo "docker build context ready:"
    ls -la {{binary_name}}-linux-*

# Build the distroless release image from local musl artifacts and smoke-test
# it. Because the base is essentially scratch (distroless/static), a passing
# `--version` here proves the binary runs on the most minimal Linux there is —
# a far stronger portability signal than any debian:slim run. Run after
# `just package-all VERSION`.
# Usage: just docker-smoke v1.2.3
docker-smoke VERSION: (docker-context VERSION)
    #!/usr/bin/env bash
    set -euo pipefail
    docker build -t {{binary_name}}:smoke .
    out="$(docker run --rm {{binary_name}}:smoke --version)"
    echo "  ✓ distroless image runs: $out"

_release-codeberg VERSION:
    #!/usr/bin/env bash
    set -euo pipefail
    api="{{codeberg_api}}/repos/{{codeberg_owner}}/{{codeberg_repo}}"
    auth="Authorization: token ${CODEBERG_TOKEN}"
    notes="$(git cliff --latest --strip header 2>/dev/null || echo 'See CHANGELOG.md for details.')"
    payload="$(jq -n --arg tag "{{VERSION}}" --arg body "$notes" \
        '{tag_name: $tag, name: $tag, body: $body, draft: false, prerelease: false}')"
    echo "Creating Codeberg release..."
    response="$(curl -sf -X POST -H "$auth" -H "Content-Type: application/json" \
        -d "$payload" "$api/releases")"
    release_id="$(echo "$response" | jq -r '.id')"
    echo "  release id: $release_id"
    mapfile -t assets < <(just _assets {{VERSION}})
    for f in "${assets[@]}"; do
        name="$(basename "$f")"
        echo "  uploading $name"
        curl -sf -X POST -H "$auth" \
            -F "attachment=@$f" \
            "$api/releases/$release_id/assets?name=$name" > /dev/null
    done

# --verify-tag: fail instead of letting gh create the tag on GitHub.
_release-github VERSION:
    #!/usr/bin/env bash
    set -euo pipefail
    mapfile -t assets < <(just _assets {{VERSION}})
    notes="$(mktemp)"
    trap 'rm -f "$notes"' EXIT
    git cliff --latest --strip header > "$notes" 2>/dev/null || echo 'See CHANGELOG.md for details.' > "$notes"
    prerelease=()
    if [[ "{{VERSION}}" == *-* ]]; then prerelease=(--prerelease); fi
    echo "Creating GitHub release..."
    gh release create {{VERSION}} "${assets[@]}" --repo {{github_repo}} --title {{VERSION}} \
        --notes-file "$notes" --verify-tag "${prerelease[@]}"
