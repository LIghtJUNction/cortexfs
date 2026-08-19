#!/usr/bin/env bash
set -Eeuo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
FORMAT=all
OUT_DIR=${CORTEXFS_PACKAGE_OUT:-$ROOT/dist}
RELEASE_DIR=${CORTEXFS_RELEASE_DIR:-}
SKIP_BUILD=0
WORK_DIR=

readonly BINARIES=(
    ctx ctxterm ctxchat tsh cortexfs-mount cortexfs-object-runner
    cortexfs-agent-runtime cortexfs-channel cortexfs-channel-tool cortexfs-channel-nostr
    cortexfs-channel-amqp cortexfs-channel-wecom-ws cortexfs-channel-wechat
    cortexfs-channel-voice cortexfs-channel-slack cortexfs-channel-mqtt ctxmcp
)
readonly UNITS=(
    cortexfs.service cortexfs-agent@.service cortexfs-agent@.socket
    cortexfs-channel@.service cortexfs-channel-bluesky.service
    cortexfs-channel-driver@.service cortexfs-channel-nostr.service
    cortexfs-channel-amqp.service cortexfs-channel-wecom-ws.service
    cortexfs-channel-wechat.service cortexfs-channel-voice.service
    cortexfs-channel-slack.service cortexfs-channel-mqtt.service
    cortexfs-channel-clawdtalk.service
    cortexfs-channel-dingtalk.service
    cortexfs-channel-email.service cortexfs-channel-gmail.service
    cortexfs-channel-irc.service cortexfs-channel-matrix.service
    cortexfs-channel-mattermost.service cortexfs-channel-mochat.service
    cortexfs-channel-notion.service
    cortexfs-channel-qq.service
    cortexfs-channel-reddit.service cortexfs-channel-twitch.service
    cortexfs-channel-twitter.service
)

fail() {
    printf 'packaging: %s\n' "$1" >&2
    exit 1
}

need() {
    command -v "$1" >/dev/null 2>&1 || fail "required command is missing: $1"
}

project_version() {
    awk '
        $0 == "[workspace.package]" { inside = 1; next }
        inside && /^\[/ { exit }
        inside && /^version[[:space:]]*=/ {
            value = $0
            sub(/^[^=]*=[[:space:]]*/, "", value)
            gsub(/["[:space:]]/, "", value)
            print value
            exit
        }
    ' "$ROOT/Cargo.toml"
}

cleanup() {
    [[ -z $WORK_DIR ]] || rm -rf -- "$WORK_DIR"
}
trap cleanup EXIT HUP INT TERM

release_dir() {
    if [[ -n $RELEASE_DIR ]]; then
        if [[ $RELEASE_DIR == /* ]]; then
            printf '%s\n' "$RELEASE_DIR"
        else
            printf '%s/%s\n' "$ROOT" "$RELEASE_DIR"
        fi
    else
        printf '%s/target/release\n' "$WORK_DIR"
    fi
}

build_release() {
    local target
    target=$(release_dir)
    if (( SKIP_BUILD )); then
        [[ -d $target ]] || fail "--skip-build requires a release directory: $target"
        return
    fi
    need cargo
    mkdir -p "$(dirname "$target")"
    (
        cd "$ROOT"
        CARGO_TARGET_DIR="$(dirname "$target")" \
        cargo build --release --locked -p cortexfs --bins -p cortexfs-mcp --bin ctxmcp \
            -p cortexfs-channel-tools \
            -p cortexfs-channel-nostr -p cortexfs-channel-amqp \
            -p cortexfs-channel-wecom-ws -p cortexfs-channel-wechat \
            -p cortexfs-channel-voice -p cortexfs-channel-slack \
            -p cortexfs-channel-mqtt
    )
}

verify_release() {
    local target binary
    target=$(release_dir)
    for binary in "${BINARIES[@]}"; do
        [[ -x $target/$binary ]] || fail "release binary is missing: $target/$binary"
    done
}

copy_source_tree() {
    local source=$1 destination=$2
    mkdir -p "$destination"
    tar -C "$source" \
        --exclude=./.git \
        --exclude=./.agents \
        --exclude=./.codensity \
        --exclude=./.codegraph \
        --exclude=./.cache \
        --exclude=./.config \
        --exclude=./.env \
        --exclude='./.env.*' \
        --exclude=./.omo \
        --exclude=./.tokensave \
        --exclude=./.bash_history \
        --exclude=./agent.sh \
        --exclude=./dist \
        --exclude=./inspect_benchmark \
        --exclude=./node_modules \
        --exclude=./docs-site \
        --exclude=./docs/assets \
        --exclude=./target \
        --exclude=./tmp \
        --exclude=./video \
        -cf - . | tar -C "$destination" -xf -
}

copy_payload() {
    local destination=$1 binary unit release=$2
    install -d -m 0755 \
        "$destination/usr/bin" \
        "$destination/usr/lib/systemd/system" \
        "$destination/usr/share/doc/cortexfs" \
        "$destination/usr/share/doc/cortexfs/docs/spec" \
        "$destination/usr/share/licenses/cortexfs" \
        "$destination/etc/cortexfs/providers.d" \
        "$destination/var/lib/cortexfs/storage/generations"
    install -d -m 0700 "$destination/var/lib/cortexfs/secrets"
    install -d -m 0700 "$destination/etc/cortexfs/channels"
    for binary in "${BINARIES[@]}"; do
        install -m 0755 "$release/$binary" "$destination/usr/bin/$binary"
    done
    for unit in "${UNITS[@]}"; do
        install -m 0644 "$ROOT/packaging/systemd/$unit" \
            "$destination/usr/lib/systemd/system/$unit"
    done
    install -m 0644 "$ROOT/README.md" "$destination/usr/share/doc/cortexfs/README.md"
    install -m 0644 "$ROOT/docs/channels.md" "$destination/usr/share/doc/cortexfs/docs/channels.md"
    install -m 0644 "$ROOT/LICENSE" "$destination/usr/share/licenses/cortexfs/LICENSE"
    install -m 0644 "$ROOT"/docs/spec/*.md \
        "$destination/usr/share/doc/cortexfs/docs/spec/"
}

stage_release() {
    local stage="$WORK_DIR/stage"
    if [[ ! -d $stage ]]; then
        copy_payload "$stage" "$(release_dir)"
    fi
    printf '%s\n' "$stage"
}

build_tar() {
    local version=$1 stage archive arch
    stage=$(stage_release)
    arch=$(uname -m)
    archive="$OUT_DIR/cortexfs-$version-linux-$arch.tar.gz"
    tar -C "$stage" -czf "$archive" .
    printf 'created %s\n' "$archive"
}

build_deb() {
    local version=$1 stage=root deb_root package architecture
    need dpkg-deb
    stage=$(stage_release)
    deb_root="$WORK_DIR/deb-root"
    cp -a "$stage" "$deb_root"
    mkdir -p "$deb_root/DEBIAN"
    chmod 0755 "$deb_root/DEBIAN"
    architecture=$(dpkg --print-architecture)
    sed -e "s/@VERSION@/$version/g" -e "s/@ARCH@/$architecture/g" \
        "$ROOT/packaging/debian/control.in" \
        >"$deb_root/DEBIAN/control"
    for package in postinst prerm postrm; do
        install -m 0755 "$ROOT/packaging/debian/$package" "$deb_root/DEBIAN/$package"
    done
    package="$OUT_DIR/cortexfs_${version}_${architecture}.deb"
    dpkg-deb --build --root-owner-group "$deb_root" "$package" >/dev/null
    printf 'created %s\n' "$package"
}

source_tree_for_rpm() {
    local version=$1 source_root="$WORK_DIR/rpm-source/cortexfs-$1"
    copy_source_tree "$ROOT" "$source_root"
    printf '%s\n' "$source_root"
}

build_rpm() {
    local version=$1 top spec rpm
    need rpmbuild
    top="$WORK_DIR/rpm"
    mkdir -p "$top"/{BUILD,BUILDROOT,RPMS,SOURCES,SPECS,SRPMS}
    source_tree_for_rpm "$version" >/dev/null
    tar -C "$WORK_DIR/rpm-source" -czf "$top/SOURCES/cortexfs-$version.tar.gz" \
        "cortexfs-$version"
    spec="$top/SPECS/cortexfs.spec"
    sed "s/^Version:.*/Version:        $version/" \
        "$ROOT/packaging/rpm/cortexfs.spec" >"$spec"
    rpmbuild -bb \
        --define "_topdir $top" \
        --define "_sourcedir $top/SOURCES" \
        --define "_builddir $top/BUILD" \
        --define "_buildrootdir $top/BUILDROOT" \
        --define "_rpmdir $top/RPMS" \
        --define "_srcrpmdir $top/SRPMS" \
        --define "_specdir $top/SPECS" "$spec" >/dev/null
    while IFS= read -r rpm; do
        cp "$rpm" "$OUT_DIR/"
        printf 'created %s\n' "$OUT_DIR/$(basename "$rpm")"
    done < <(find "$top/RPMS" -type f -name "cortexfs-$version-*.rpm" -print)
}

build_arch() {
    local version=$1 arch_dir package
    need makepkg
    arch_dir="$WORK_DIR/arch"
    mkdir -p "$arch_dir"
    cp "$ROOT/packaging/arch/PKGBUILD" "$ROOT/packaging/arch/cortexfs.install" "$arch_dir/"
    (
        cd "$arch_dir"
        CORTEXFS_PKGVER="$version" \
            CORTEXFS_SOURCE_DIR="$ROOT" \
            CORTEXFS_PREBUILT_DIR="$(stage_release)" \
            makepkg --cleanbuild --force --noconfirm
    )
    while IFS= read -r package; do
        cp "$package" "$OUT_DIR/"
        printf 'created %s\n' "$OUT_DIR/$(basename "$package")"
    done < <(find "$arch_dir" -maxdepth 1 -type f -name '*.pkg.tar.*' -print)
}

usage() {
    cat <<'USAGE'
Usage: packaging/build.sh [options]

Options:
  --format FORMAT       deb, rpm, arch, tar, or all (default: all)
  --out DIR             output directory (default: ./dist)
  --release-dir DIR     use existing target/release binaries
  --skip-build          do not run cargo build; requires --release-dir
  -h, --help            show this help
USAGE
}

main() {
    local version
    while (($#)); do
        case "$1" in
            --format) FORMAT=${2:?--format requires a value}; shift 2 ;;
            --out) OUT_DIR=${2:?--out requires a value}; shift 2 ;;
            --release-dir) RELEASE_DIR=${2:?--release-dir requires a value}; shift 2 ;;
            --skip-build) SKIP_BUILD=1; shift ;;
            -h|--help) usage; return 0 ;;
            *) usage >&2; fail "unknown option: $1" ;;
        esac
    done
    [[ $FORMAT == deb || $FORMAT == rpm || $FORMAT == arch || $FORMAT == tar || $FORMAT == all ]] ||
        fail "invalid format: $FORMAT"
    [[ $SKIP_BUILD -eq 0 || -n $RELEASE_DIR ]] ||
        fail "--skip-build requires --release-dir"
    version=$(project_version)
    [[ -n $version ]] || fail "cannot read workspace package version"
    mkdir -p "$OUT_DIR"
    WORK_DIR=$(mktemp -d "${TMPDIR:-/tmp}/cortexfs-package.XXXXXX")
    if [[ $FORMAT == rpm ]]; then
        build_rpm "$version"
        return
    fi
    build_release
    verify_release
    case "$FORMAT" in
        deb) build_deb "$version" ;;
        arch) build_arch "$version" ;;
        tar) build_tar "$version" ;;
        all)
            build_deb "$version"
            build_rpm "$version"
            build_arch "$version"
            build_tar "$version"
            ;;
    esac
}

main "$@"
