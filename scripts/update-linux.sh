#!/bin/bash
set -Eeuo pipefail

readonly UPDATE_REPOSITORY=https://github.com/LIghtJUNction/cortexfs.git
readonly UPDATE_PROTOCOL=cortexfs.update/v1
readonly UPDATE_STATE_FILE=/var/lib/cortexfs/update-state
readonly UPDATE_ROOT=/var/lib/cortexfs/updates
PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
UPDATE_USER_HOME=$(getent passwd "$(id -u)" | cut -d: -f6)
[[ -n $UPDATE_USER_HOME ]] || {
    printf 'ctx update: cannot resolve the user home\n' >&2
    exit 1
}
HOME=$UPDATE_USER_HOME
PATH="$HOME/.cargo/bin:$PATH"
export HOME PATH
UPDATE_REF=
UPDATE_SOURCE=
UPDATE_APPLY=0
UPDATE_TEMP=
UPDATE_TXN=
UPDATE_OWNER=
UPDATE_BACKEND=
UPDATE_STORAGE_TARGET=-
UPDATE_SWITCHED=0

update_validate_installed_helper() {
    local helper=/usr/lib/cortexfs/update-linux owner mode parent_owner parent_mode
    [[ $0 == "$helper" ]] || return 0
    [[ -f $helper && ! -L $helper && -x $helper && -d ${helper%/*} && ! -L ${helper%/*} ]] ||
        update_fail 'installed updater path is not plain'
    read -r owner mode < <(stat -c '%u %a' "$helper")
    read -r parent_owner parent_mode < <(stat -c '%u %a' "${helper%/*}")
    [[ $owner == 0 && $parent_owner == 0 ]] || update_fail 'installed updater is not root-owned'
    (((8#$mode & 8#022) == 0 && (8#$parent_mode & 8#022) == 0)) ||
        update_fail 'installed updater path is writable by an untrusted user'
}

update_usage() {
    cat <<'USAGE'
Usage: ctx update [--ref REF | --source PATH] [--yes]

Resolve an immutable Git commit, build its native Linux package, and print the
update plan. --yes applies that plan with health checks and automatic rollback.
The first update requires --ref or --source; later updates reuse the saved ref.
USAGE
}

update_fail() {
    printf 'ctx update: %s\n' "$1" >&2
    exit "${2:-1}"
}

update_parse() {
    while (($#)); do
        case "$1" in
        --ref)
            [[ $# -ge 2 ]] || update_fail '--ref requires a value' 2
            UPDATE_REF=$2
            shift 2
            ;;
        --source)
            [[ $# -ge 2 ]] || update_fail '--source requires a path' 2
            UPDATE_SOURCE=$2
            shift 2
            ;;
        --yes)
            UPDATE_APPLY=1
            shift
            ;;
        -h | --help)
            update_usage
            exit 0
            ;;
        *)
            update_usage >&2
            update_fail "unknown option: $1" 2
            ;;
        esac
    done
    [[ -z $UPDATE_REF || -z $UPDATE_SOURCE ]] ||
        update_fail '--ref and --source are mutually exclusive' 2
}

update_valid_ref() {
    [[ $1 =~ ^[A-Za-z0-9][A-Za-z0-9._/@:+~-]{0,199}$ ]]
}

update_saved_ref() {
    [[ -f $UPDATE_STATE_FILE && ! -L $UPDATE_STATE_FILE ]] || return 1
    awk -F= '$1 == "tracking_ref" && $2 != "-" { print substr($0, index($0, "=") + 1); exit }' \
        "$UPDATE_STATE_FILE"
}

update_resolve_target() {
    if [[ -n $UPDATE_SOURCE ]]; then
        command -v git >/dev/null 2>&1 || update_fail 'git is required for --source'
        UPDATE_SOURCE=$(realpath -- "$UPDATE_SOURCE")
        if printf '%s' "$UPDATE_SOURCE" | LC_ALL=C grep -q '[[:cntrl:]]'; then
            update_fail '--source path contains a control character'
        fi
        [[ $(git -C "$UPDATE_SOURCE" rev-parse --show-toplevel 2>/dev/null || true) == "$UPDATE_SOURCE" ]] ||
            update_fail '--source must be the root of a Git checkout'
        [[ -z $(git -C "$UPDATE_SOURCE" status --porcelain --untracked-files=normal) ]] ||
            update_fail '--source must be a clean Git checkout; commit or stash changes first'
    else
        if [[ -z $UPDATE_REF ]]; then
            UPDATE_REF=$(update_saved_ref) ||
                update_fail 'the first update requires --ref REF or --source PATH' 2
        fi
        update_valid_ref "$UPDATE_REF" || update_fail 'invalid Git ref' 2
        command -v git >/dev/null 2>&1 || update_fail 'git is required for --ref'
        UPDATE_SOURCE=$UPDATE_TEMP/source
        git init --quiet "$UPDATE_SOURCE"
        git -C "$UPDATE_SOURCE" remote add origin "$UPDATE_REPOSITORY"
        git -C "$UPDATE_SOURCE" fetch --quiet --depth=1 origin "$UPDATE_REF"
        git -C "$UPDATE_SOURCE" checkout --quiet --detach FETCH_HEAD
    fi
    UPDATE_REVISION=$(git -C "$UPDATE_SOURCE" rev-parse --verify HEAD)
    [[ $UPDATE_REVISION =~ ^[0-9a-f]{40}$ ]] || update_fail 'cannot resolve an immutable commit'
    [[ -f $UPDATE_SOURCE/packaging/update-protocol &&
        ! -L $UPDATE_SOURCE/packaging/update-protocol &&
        $(<"$UPDATE_SOURCE/packaging/update-protocol") == "$UPDATE_PROTOCOL" ]] ||
        update_fail "target does not support $UPDATE_PROTOCOL"
    [[ -x $UPDATE_SOURCE/packaging/build.sh && ! -L $UPDATE_SOURCE/packaging/build.sh ]] ||
        update_fail 'target packaging entrypoint is missing or unsafe'
    for entrypoint in scripts/install-linux.sh scripts/update-linux.sh; do
        [[ -f $UPDATE_SOURCE/$entrypoint && ! -L $UPDATE_SOURCE/$entrypoint ]] ||
            update_fail "target entrypoint is missing or unsafe: $entrypoint"
    done
}

update_backend() {
    local id like
    id=$(awk -F= '$1 == "ID" { gsub(/^['\''"]|['\''"]$/, "", $2); print tolower($2); exit }' /etc/os-release)
    like=$(awk -F= '$1 == "ID_LIKE" { value=substr($0,index($0,"=")+1); gsub(/^['\''"]|['\''"]$/, "", value); print tolower(value); exit }' /etc/os-release)
    case " $id $like " in
    *' arch '* | *' manjaro '*) printf 'arch\n' ;;
    *' debian '* | *' ubuntu '*) printf 'deb\n' ;;
    *' fedora '* | *' rhel '* | *' centos '* | *' rocky '* | *' almalinux '* | *' opensuse '* | *' suse '* | *' sles '*) printf 'rpm\n' ;;
    *) return 1 ;;
    esac
}

update_owner() {
    if command -v dpkg-query >/dev/null 2>&1 && dpkg-query -W cortexfs >/dev/null 2>&1; then
        printf 'deb\n'
    elif command -v rpm >/dev/null 2>&1 && rpm -q cortexfs >/dev/null 2>&1; then
        printf 'rpm\n'
    elif command -v pacman >/dev/null 2>&1 && pacman -Q cortexfs >/dev/null 2>&1; then
        printf 'arch\n'
    else
        printf 'source\n'
    fi
}

update_current_revision() {
    [[ -f $UPDATE_STATE_FILE && ! -L $UPDATE_STATE_FILE ]] || {
        printf 'unknown\n'
        return
    }
    awk -F= '$1 == "revision" { print $2; found=1; exit } END { if (!found) print "unknown" }' \
        "$UPDATE_STATE_FILE"
}

update_plan() {
    UPDATE_BACKEND=$(update_backend) || update_fail 'unsupported Linux distribution'
    UPDATE_OWNER=$(update_owner)
    printf 'current_revision\t%s\n' "$(update_current_revision)"
    printf 'target_revision\t%s\n' "$UPDATE_REVISION"
    if [[ -n $UPDATE_REF ]]; then
        printf 'source\tref:%s\n' "$UPDATE_REF"
    else
        printf 'source\tcheckout:%s\n' "$UPDATE_SOURCE"
    fi
    printf 'backend\t%s\n' "$UPDATE_BACKEND"
    printf 'installed_as\t%s\n' "$UPDATE_OWNER"
    if ((!UPDATE_APPLY)); then
        printf 'action\tplan-only (rerun with --yes to apply)\n'
    fi
}

update_package_paths() {
    case "$1" in
    deb) dpkg-deb --fsys-tarfile "$2" | tar -tf - ;;
    rpm) rpm -qlp "$2" ;;
    arch) bsdtar -tf "$2" ;;
    esac
}

update_path_allowed() {
    case "$1" in
    . | .BUILDINFO | .INSTALL | .MTREE | .PKGINFO | usr | usr/bin | usr/lib | usr/lib/.build-id | usr/lib/systemd | usr/lib/systemd/system | usr/lib/cortexfs | usr/share | usr/share/doc | usr/share/doc/cortexfs | usr/share/licenses | usr/share/licenses/cortexfs | etc | etc/cortexfs | etc/cortexfs/providers.d | etc/cortexfs/channels | var | var/lib | var/lib/cortexfs | var/lib/cortexfs/storage | var/lib/cortexfs/storage/generations | var/lib/cortexfs/secrets) ;;
    usr/bin/ctx | usr/bin/ctxterm | usr/bin/ctxchat | usr/bin/ctxmcp | usr/bin/tsh | usr/bin/cortexfs-*) ;;
    usr/lib/.build-id/* | usr/lib/systemd/system/cortexfs.service | usr/lib/systemd/system/cortexfs-*) ;;
    usr/lib/cortexfs/update-linux) ;;
    usr/share/doc/cortexfs/* | usr/share/licenses/cortexfs/*) ;;
    *) return 1 ;;
    esac
}

update_verify_package() {
    local package=$1 paths=$UPDATE_TEMP/package-paths path normalized
    update_package_paths "$UPDATE_BACKEND" "$package" >"$paths"
    while IFS= read -r path; do
        normalized=${path#./}
        normalized=${normalized#/}
        normalized=${normalized%/}
        [[ $normalized != *'/../'* && $normalized != ../* ]] ||
            update_fail "package contains an unsafe path: $path"
        update_path_allowed "${normalized:-.}" ||
            update_fail "package contains an unmanaged path: $path"
    done <"$paths"
    sed -e 's#^\./##' -e 's#^/##' -e 's#/$##' "$paths" | grep -Fxq usr/bin/ctx ||
        update_fail 'package does not contain /usr/bin/ctx'
    sed -e 's#^\./##' -e 's#^/##' -e 's#/$##' "$paths" | grep -Fxq usr/lib/cortexfs/update-linux ||
        update_fail 'package does not contain the updater'
}

update_build_package() {
    local out=$UPDATE_TEMP/package pattern
    mkdir -p "$out"
    "$UPDATE_SOURCE/packaging/build.sh" --format "$UPDATE_BACKEND" --out "$out"
    case "$UPDATE_BACKEND" in
    deb)
        pattern='cortexfs_*.deb'
        UPDATE_CANDIDATE_NAME=candidate.deb
        ;;
    rpm)
        pattern='cortexfs-*.rpm'
        UPDATE_CANDIDATE_NAME=candidate.rpm
        ;;
    arch)
        pattern='cortexfs-*.pkg.tar.*'
        UPDATE_CANDIDATE_NAME=candidate.pkg.tar.zst
        ;;
    esac
    mapfile -t packages < <(find "$out" -maxdepth 1 -type f -name "$pattern" -print)
    [[ ${#packages[@]} -eq 1 ]] || update_fail 'packaging did not produce exactly one native package'
    UPDATE_PACKAGE=${packages[0]}
    update_verify_package "$UPDATE_PACKAGE"
    UPDATE_PACKAGE_SHA=$(sha256sum "$UPDATE_PACKAGE" | awk '{print $1}')
}

update_active_units() {
    systemctl list-units --all --state=active --type=service --type=socket \
        'cortexfs*' --no-legend --plain 2>/dev/null |
        awk '$1 ~ /^cortexfs[-A-Za-z0-9@.]*\.(service|socket)$/ { print $1 }' | sort -u
}

update_installed_version() {
    case "$UPDATE_OWNER" in
    deb) dpkg-query -W -f='${Version}\n' cortexfs ;;
    rpm) rpm -q --qf '%{VERSION}-%{RELEASE}.%{ARCH}\n' cortexfs ;;
    arch) pacman -Q cortexfs | awk '{print $2}' ;;
    esac
}

update_package_version() {
    case "$UPDATE_OWNER" in
    deb) dpkg-deb -f "$1" Version ;;
    rpm) rpm -qp --qf '%{VERSION}-%{RELEASE}.%{ARCH}\n' "$1" ;;
    arch) pacman -Qp "$1" | awk '{print $2}' ;;
    esac
}

update_package_matches_install() {
    local package=$1 extracted=$UPDATE_TEMP/rollback-extracted path relative
    rm -rf -- "$extracted"
    mkdir -p "$extracted"
    case "$UPDATE_OWNER" in
    deb) dpkg-deb -x "$package" "$extracted" ;;
    rpm) (cd "$extracted" && rpm2cpio "$package" | cpio -idm --quiet) ;;
    arch) bsdtar -xf "$package" -C "$extracted" ;;
    esac
    rm -f -- "$extracted"/.BUILDINFO "$extracted"/.INSTALL "$extracted"/.MTREE "$extracted"/.PKGINFO
    while IFS= read -r path; do
        relative=${path#"$extracted"/}
        if [[ -L $path ]]; then
            [[ -L /$relative && $(readlink "$path") == "$(readlink "/$relative")" ]] || return 1
        else
            cmp -s -- "$path" "/$relative" || return 1
        fi
    done < <(find "$extracted" \( -type f -o -type l \) -print)
}

update_find_rollback_package() {
    local version candidate check=$UPDATE_TEMP/rollback-package pattern extension
    local -a roots
    version=$(update_installed_version)
    case "$UPDATE_OWNER" in
    deb)
        roots=(/var/cache/apt/archives /var/lib/cortexfs/deploy /var/lib/cortexfs/updates)
        pattern='cortexfs_*.deb'
        extension=deb
        ;;
    rpm)
        roots=(/var/cache/dnf /var/cache/yum /var/cache/zypp /var/lib/cortexfs/updates)
        pattern='cortexfs-*.rpm'
        extension=rpm
        ;;
    arch)
        roots=(/var/cache/pacman/pkg /var/lib/cortexfs/updates)
        pattern='cortexfs-*.pkg.tar.*'
        extension=pkg.tar.zst
        ;;
    esac
    while IFS= read -r candidate; do
        sudo install -m 0644 "$candidate" "$check"
        if [[ $(update_package_version "$check" 2>/dev/null || true) == "$version" ]] &&
            update_package_matches_install "$check"; then
            UPDATE_ROLLBACK_PACKAGE=$check
            UPDATE_ROLLBACK_EXTENSION=$extension
            return 0
        fi
    done < <(sudo find "${roots[@]}" -type f -name "$pattern" -print 2>/dev/null | sort -r)
    update_fail "cannot find an exact installed $UPDATE_OWNER package for rollback (version $version)"
}

update_source_artifacts() {
    local path
    for path in /usr/bin/ctx /usr/bin/ctxterm /usr/bin/ctxchat /usr/bin/ctxmcp /usr/bin/tsh \
        /usr/lib/cortexfs/update-linux /usr/share/doc/cortexfs /usr/share/licenses/cortexfs \
        /var/lib/cortexfs/install-state; do
        sudo test ! -e "$path" || printf '%s\n' "${path#/}"
    done
    sudo find /usr/bin -maxdepth 1 -type f -name 'cortexfs-*' -printf '%p\n' 2>/dev/null | sed 's#^/##'
    sudo find /usr/lib/systemd/system -maxdepth 1 -type f -name 'cortexfs-*' -printf '%p\n' 2>/dev/null | sed 's#^/##'
}

update_write_txn_state() {
    local phase=$1 local_state=$UPDATE_TEMP/transaction-state staged=$UPDATE_TXN/.state-new
    printf 'schema=1\nphase=%s\nowner=%s\nbackend=%s\nstorage_target=%s\n' \
        "$phase" "$UPDATE_OWNER" "$UPDATE_BACKEND" "$UPDATE_STORAGE_TARGET" >"$local_state"
    sudo install -m 0600 "$local_state" "$staged"
    sudo mv -f "$staged" "$UPDATE_TXN/state"
}

update_state_field() {
    awk -F= -v key="$1" '$1 == key { print substr($0, index($0, "=") + 1); found=1; exit } END { if (!found) exit 1 }' "$2"
}

update_recover_pending() {
    local link state phase schema
    sudo test -L "$UPDATE_ROOT/current" || return 0
    link=$(sudo readlink "$UPDATE_ROOT/current")
    [[ $link =~ ^[A-Za-z0-9._-]+$ ]] || update_fail 'pending transaction link is unsafe'
    UPDATE_TXN=$UPDATE_ROOT/$link
    if ! sudo test -d "$UPDATE_TXN" || sudo test -L "$UPDATE_TXN"; then
        update_fail 'pending transaction directory is unsafe'
    fi
    state=$UPDATE_TEMP/pending-state
    # shellcheck disable=SC2024 # Read a root-only file into a user-owned temporary file.
    sudo cat "$UPDATE_TXN/state" >"$state"
    schema=$(update_state_field schema "$state")
    phase=$(update_state_field phase "$state")
    UPDATE_OWNER=$(update_state_field owner "$state")
    UPDATE_BACKEND=$(update_state_field backend "$state")
    UPDATE_STORAGE_TARGET=$(update_state_field storage_target "$state")
    [[ $schema == 1 && $UPDATE_OWNER =~ ^(deb|rpm|arch|source)$ &&
        $UPDATE_BACKEND =~ ^(deb|rpm|arch)$ &&
        ($UPDATE_STORAGE_TARGET == - || $UPDATE_STORAGE_TARGET == generations/*) ]] ||
        update_fail 'pending transaction state is invalid'
    # shellcheck disable=SC2024 # Read a root-only file into a user-owned temporary file.
    sudo cat "$UPDATE_TXN/active-units" >"$UPDATE_TEMP/active-units"
    case "$phase" in
    committed | rolled-back | abandoned)
        sudo rm -f "$UPDATE_ROOT/current"
        ;;
    prepared)
        update_write_txn_state abandoned
        sudo rm -f "$UPDATE_ROOT/current"
        ;;
    installing)
        UPDATE_SWITCHED=1
        update_rollback
        ;;
    *) update_fail 'pending transaction phase is invalid' ;;
    esac
}

update_prepare_transaction() {
    local name relative
    local -a artifacts config_paths
    name="$(date -u +%Y%m%dT%H%M%SZ)-${UPDATE_REVISION:0:12}-$$"
    UPDATE_TXN=$UPDATE_ROOT/$name
    sudo install -d -m 0755 "$UPDATE_ROOT"
    if sudo test -e "$UPDATE_ROOT/current" || sudo test -L "$UPDATE_ROOT/current"; then
        update_fail 'an unfinished update requires recovery'
    fi
    [[ $UPDATE_OWNER == source ]] || update_package_matches_install "$UPDATE_ROLLBACK_PACKAGE" ||
        update_fail 'installed files changed after rollback-package verification'
    sudo install -d -m 0700 "$UPDATE_TXN"
    update_active_units >"$UPDATE_TEMP/active-units"
    sudo install -m 0600 "$UPDATE_TEMP/active-units" "$UPDATE_TXN/active-units"
    UPDATE_STORAGE_TARGET=$(sudo readlink /var/lib/cortexfs/storage/current 2>/dev/null || printf '%s' -)
    [[ $UPDATE_STORAGE_TARGET == - || $UPDATE_STORAGE_TARGET == generations/* ]] ||
        update_fail 'storage/current has an unsafe target'
    printf '%s\n' "$UPDATE_STORAGE_TARGET" >"$UPDATE_TEMP/storage-target"
    sudo install -m 0600 "$UPDATE_TEMP/storage-target" "$UPDATE_TXN/storage-target"
    if [[ $UPDATE_OWNER == source ]]; then
        mapfile -t artifacts < <(update_source_artifacts | sort -u)
        ((${#artifacts[@]})) || update_fail 'no installed CortexFS artifacts found for rollback'
        sudo tar -C / -czf "$UPDATE_TXN/rollback.tar.gz" "${artifacts[@]}"
    else
        sudo install -m 0644 "$UPDATE_ROLLBACK_PACKAGE" \
            "$UPDATE_TXN/rollback.$UPDATE_ROLLBACK_EXTENSION"
    fi
    sudo install -m 0644 "$UPDATE_PACKAGE" "$UPDATE_TXN/$UPDATE_CANDIDATE_NAME"
    printf '%s  %s\n' "$UPDATE_PACKAGE_SHA" "$UPDATE_CANDIDATE_NAME" >"$UPDATE_TEMP/candidate.sha256"
    sudo install -m 0600 "$UPDATE_TEMP/candidate.sha256" "$UPDATE_TXN/candidate.sha256"
    config_paths=()
    for relative in etc/cortexfs var/lib/cortexfs/secrets var/lib/cortexfs/install-state; do
        sudo test ! -e "/$relative" || config_paths+=("$relative")
    done
    ((${#config_paths[@]} == 0)) || sudo tar -C / -czf "$UPDATE_TXN/config.tar.gz" "${config_paths[@]}"
    update_write_txn_state prepared
    sudo ln -s "$name" "$UPDATE_ROOT/current" ||
        update_fail 'another update created a transaction first'
}

update_install_package() {
    local backend=$1 package=$2
    case "$backend" in
    deb) sudo env CORTEXFS_UPDATE_TRANSACTION=1 dpkg --install "$package" ;;
    rpm) sudo env CORTEXFS_UPDATE_TRANSACTION=1 rpm --upgrade --replacepkgs --oldpackage "$package" ;;
    arch) sudo env CORTEXFS_UPDATE_TRANSACTION=1 pacman --upgrade --noconfirm "$package" ;;
    esac
}

update_restart_units() {
    local unit
    local -a units current_units
    mapfile -t units <"$UPDATE_TEMP/active-units"
    mapfile -t current_units < <(update_active_units)
    for unit in "${current_units[@]}"; do
        grep -Fxq "$unit" "$UPDATE_TEMP/active-units" || sudo systemctl stop "$unit"
    done
    ((${#units[@]} == 0)) || sudo systemctl restart "${units[@]}"
}

update_restore_storage() {
    local temporary=/var/lib/cortexfs/storage/.current-update-$$
    if [[ $UPDATE_STORAGE_TARGET == - ]]; then
        sudo rm -f /var/lib/cortexfs/storage/current
    else
        sudo ln -s "$UPDATE_STORAGE_TARGET" "$temporary"
        sudo mv -Tf "$temporary" /var/lib/cortexfs/storage/current
    fi
}

update_verify() {
    local unit
    /usr/bin/ctx update --help >/dev/null
    while IFS= read -r unit; do
        systemctl is-active --quiet "$unit" || update_fail "unit did not recover: $unit"
    done <"$UPDATE_TEMP/active-units"
    if grep -Fxq cortexfs.service "$UPDATE_TEMP/active-units"; then
        findmnt -rnM /ctx >/dev/null || update_fail '/ctx is not mounted after update'
        sudo /usr/bin/ctx status >/dev/null || update_fail 'ctx status failed after update'
    fi
}

update_write_state() {
    local state=$UPDATE_TEMP/update-state tracking=${UPDATE_REF:--} staged=${UPDATE_STATE_FILE}.new.$$
    printf 'schema=1\nrevision=%s\ntracking_ref=%s\nbackend=%s\nupdated_at=%s\n' \
        "$UPDATE_REVISION" "$tracking" "$UPDATE_BACKEND" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" >"$state"
    sudo install -m 0644 "$state" "$staged"
    sudo mv -f "$staged" "$UPDATE_STATE_FILE"
}

update_rollback() {
    local status=0 package
    printf 'ctx update: health check failed; restoring the previous release\n' >&2
    set +e
    mapfile -t units <"$UPDATE_TEMP/active-units"
    ((${#units[@]} == 0)) || sudo systemctl stop "${units[@]}"
    update_restore_storage || status=1
    if [[ $UPDATE_OWNER == source ]]; then
        case "$UPDATE_BACKEND" in
        deb) sudo env CORTEXFS_UPDATE_TRANSACTION=1 dpkg --remove cortexfs ;;
        rpm) sudo env CORTEXFS_UPDATE_TRANSACTION=1 rpm --erase cortexfs ;;
        arch) sudo env CORTEXFS_UPDATE_TRANSACTION=1 pacman --remove --noconfirm cortexfs ;;
        esac
        sudo tar -C / -xzf "$UPDATE_TXN/rollback.tar.gz" || status=1
    else
        package=$(find "$UPDATE_TXN" -maxdepth 1 -type f -name 'rollback.*' -print -quit)
        update_install_package "$UPDATE_OWNER" "$package" || status=1
    fi
    sudo systemctl daemon-reload || status=1
    update_restart_units || status=1
    update_write_txn_state rolled-back || status=1
    sudo rm -f "$UPDATE_ROOT/current" || status=1
    UPDATE_SWITCHED=0
    set -e
    if ((status)); then
        printf 'ctx update: rollback incomplete; recovery files remain at %s\n' "$UPDATE_TXN" >&2
        return 1
    fi
    printf 'ctx update: rollback completed from %s\n' "$UPDATE_TXN" >&2
}

update_exit() {
    local status=$?
    trap - EXIT HUP INT TERM
    if ((status != 0 && UPDATE_SWITCHED)); then
        update_rollback || status=1
    fi
    [[ -z $UPDATE_TEMP ]] || rm -rf -- "$UPDATE_TEMP"
    exit "$status"
}

update_signal() {
    exit 130
}

update_begin_apply() {
    [[ ${EUID:-$(id -u)} -ne 0 ]] || update_fail 'run ctx update as a normal user with sudo'
    command -v sudo flock >/dev/null 2>&1 || update_fail 'sudo and flock are required'
    [[ -r /dev/tty && -w /dev/tty ]] || update_fail 'a controlling terminal is required'
    install -d -m 0700 "$HOME/.cache/cortexfs"
    exec 9>"$HOME/.cache/cortexfs/update.lock"
    flock -n 9 || update_fail 'another update is already running'
    # shellcheck disable=SC2024 # Force sudo's prompt onto the controlling terminal.
    sudo -v </dev/tty
    update_recover_pending
}

update_apply() {
    local distro family manager id
    export CORTEXFS_INSTALL_LIB=1 CORTEXFS_ASSUME_YES=1
    # shellcheck disable=SC1090,SC1091 # The pinned checkout is selected at runtime.
    source "$UPDATE_SOURCE/scripts/install-linux.sh"
    trap update_exit EXIT
    trap update_signal HUP INT TERM
    export TEMP_DIR=$UPDATE_TEMP
    setup_style
    check_host
    resolve_language_and_install_kind
    validate_source "$UPDATE_SOURCE"
    distro=$(detect_distro /etc/os-release "$(command_list)") || update_fail 'unsupported package manager'
    IFS='|' read -r family manager id <<<"$distro"
    install_dependencies "$family" "$manager"
    require_runtime_paths
    audit_fuse
    ensure_rust
    check_bwrap
    ensure_mountpoint

    [[ $UPDATE_OWNER == source ]] || update_find_rollback_package
    update_build_package
    update_prepare_transaction
    update_write_txn_state installing
    UPDATE_SWITCHED=1
    update_install_package "$UPDATE_BACKEND" "$UPDATE_TXN/$UPDATE_CANDIDATE_NAME"
    sudo systemctl daemon-reload
    update_restart_units
    update_verify
    update_write_state
    update_write_txn_state committed
    sudo rm -f "$UPDATE_ROOT/current"
    UPDATE_SWITCHED=0
    printf 'updated\t%s\npackage_sha256\t%s\ntransaction\t%s\n' \
        "$UPDATE_REVISION" "$UPDATE_PACKAGE_SHA" "$UPDATE_TXN"
}

main() {
    update_validate_installed_helper
    update_parse "$@"
    trap update_exit EXIT
    trap update_signal HUP INT TERM
    UPDATE_TEMP=$(mktemp -d "${TMPDIR:-/tmp}/cortexfs-update.XXXXXX")
    ((!UPDATE_APPLY)) || update_begin_apply
    update_resolve_target
    update_plan
    ((UPDATE_APPLY)) || return 0
    update_apply
}

if [[ ${CORTEXFS_UPDATE_LIB:-0} != 1 ]]; then
    main "$@"
fi
