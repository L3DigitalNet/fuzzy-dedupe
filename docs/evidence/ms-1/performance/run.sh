#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 L3Digital LLC
# SPDX-License-Identifier: GPL-3.0-only
#
# Run the spec §11.9 measurement scenarios T1 to T4 for plan task T25
# (REQ-001, PV-T25-001) and collect their C-10 sample streams.
#
# Usage:
#   docs/evidence/ms-1/performance/run.sh --go [options]
#   docs/evidence/ms-1/performance/run.sh --dry-run [options]
#
# Options:
#   --go                    start the measured runs (required; nothing runs
#                           without it)
#   --dry-run               print the run conditions and the run plan; launch
#                           nothing and write nothing
#   --bin PATH              the release binary (default:
#                           <repo>/target/release/fuzzy-dedupe)
#   --runs N                runs per scenario, at least 3 (default 3)
#   --secs N                length of each run in seconds, at least 60
#                           (default 60)
#   --pause N               idle seconds between runs (default 5)
#   --replace               delete an earlier run's files in raw/ first
#   --allow-small-display   run even when no enabled output's logical area
#                           holds the 1600 x 960 reference client area
#
# Contract. With --go, the script records the run conditions to
# raw/conditions.txt, then runs every scenario --runs times, one process at a
# time, each as `FUZZY_DEDUPE_MEASURE=<t>,run=<n>,secs=<secs> <bin>` with
# standard output redirected to raw/<t>-run<n>.csv. It appends one line per
# finished run to raw/runs.tsv (scenario, run, configured and wall seconds,
# load average before the run). It exits non-zero, and stops at once, on any
# refused precondition, any run that exits non-zero, or any stream that is
# missing, empty, lacks the C-10 header or carries no sample: C-10 says a
# missing or empty sample stream fails the run. analyze.py then reads raw/.
#
# Run order is round-robin (run 1 of T1 to T4, then run 2, ...), not
# scenario by scenario, so slow drift over the session (thermal state,
# background load) spreads over every scenario instead of biasing the one
# that happened to run last.
#
# A stream is written to <file>.partial and renamed only after it passes the
# checks, so a failed or interrupted run never leaves a complete-looking CSV
# for analyze.py to pool with the good ones.
#
# Privacy (plan §2 evidence rule): conditions.txt must carry no user name,
# host name or home path. Every recorded line passes through `redact`, and
# the finished text is searched for all three before it is written; a hit
# aborts the run rather than recording it.
#
# SCOPE: the script writes only under raw/ beside it. The app's own
# preferences.toml is neither read nor written by a measurement run (T23,
# `FrameHook::isolates_preferences`); its log directory is the app's
# business and is not collected here.
#
# Requirements: bash 4 or newer (`${var//pattern/replacement}`, arrays);
# GNU coreutils (`timeout`, `sha256sum`, `nproc`, `mv -T`) and binutils
# `readelf`, used for the release-build check; git, for the C-14 binding;
# /usr/bin/python3 with only the standard library, to read kscreen-doctor's
# JSON. lscpu, plasmashell, vulkaninfo or glxinfo, kscreen-doctor, rpm,
# powerprofilesctl and tuned-adm are optional: a missing one is recorded as
# "unavailable", never fatal, because only the binary and the streams decide
# whether a run is valid.

set -euo pipefail

# As measure::recorder::HEADER writes it; analyze.py checks the same line.
readonly C10_HEADER='scenario,run,metric,value_ms'
readonly SCENARIOS=(t1 t2 t3 t4)
# C-10 minimums; --runs and --secs may raise them, never lower them.
readonly MIN_RUNS=3
readonly MIN_SECS=60
# The §11.3 reference client area, in logical pixels.
readonly REF_WIDTH=1600
readonly REF_HEIGHT=960

die() {
	printf 'run.sh: %s\n' "$*" >&2
	exit 1
}

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
repo_root=$(git -C "$script_dir" rev-parse --show-toplevel) ||
	die "not inside a git checkout; the C-14 binding needs one"
raw_dir="$script_dir/raw"

go=false
dry_run=false
replace=false
allow_small_display=false
bin="$repo_root/target/release/fuzzy-dedupe"
runs=$MIN_RUNS
secs=$MIN_SECS
pause=5

while (($# > 0)); do
	case $1 in
	--go) go=true ;;
	--dry-run) dry_run=true ;;
	--replace) replace=true ;;
	--allow-small-display) allow_small_display=true ;;
	--bin | --runs | --secs | --pause)
		(($# >= 2)) || die "$1 needs a value"
		case $1 in
		--bin) bin=$2 ;;
		--runs) runs=$2 ;;
		--secs) secs=$2 ;;
		--pause) pause=$2 ;;
		esac
		shift
		;;
	-h | --help)
		sed -n '/^# Usage:/,/^# Contract/{/^# Contract/d;s/^# \{0,1\}//;p}' "${BASH_SOURCE[0]}"
		exit 0
		;;
	*) die "unknown argument: $1 (see --help)" ;;
	esac
	shift
done

# --go is the owner's explicit consent to open measurement windows; nothing
# below launches the app without it, and --dry-run never does.
if $go && $dry_run; then
	die "--go and --dry-run exclude each other"
fi
if ! $go && ! $dry_run; then
	die "refusing to start without --go (or --dry-run to only print the conditions)"
fi

if ! [[ $runs =~ ^[0-9]+$ ]] || ((runs < MIN_RUNS)); then
	die "--runs must be an integer of at least $MIN_RUNS (C-10)"
fi
if ! [[ $secs =~ ^[0-9]+$ ]] || ((secs < MIN_SECS)); then
	die "--secs must be an integer of at least $MIN_SECS (C-10)"
fi
[[ $pause =~ ^[0-9]+$ ]] || die "--pause must be a non-negative integer"

# ── Release-build check ──────────────────────────────────────────────────
# §11.9 targets bind a release build. Two independent signs, both required:
# Cargo puts release artifacts under target/release/, and the workspace's
# release profile is Cargo's default (no [profile.release] in Cargo.toml),
# which builds without debug info and, since Rust 1.77, strips the std
# debug info too, so a release binary has no .debug_* section while a dev
# build always has one. A path check alone would accept a debug binary
# copied there; a section check alone would accept a stripped dev build
# from anywhere. If the release profile ever enables `debug`, this check
# must change with it.
[[ -f $bin && -x $bin ]] || die "no executable binary at $bin"
bin=$(cd -- "$(dirname -- "$bin")" && pwd -P)/$(basename -- "$bin")
[[ $bin == */target/release/* ]] ||
	die "$bin is not under a target/release/ directory; build it with cargo build --release"
# readelf must parse the file first: on a non-ELF file it prints no
# sections, which the grep below would read as "no debug info".
sections=$(readelf -S --wide -- "$bin" 2>/dev/null) ||
	die "$bin is not an ELF executable readelf can read"
if grep -q '\.debug_' <<<"$sections"; then
	die "$bin has debug sections, so it is not a default release build"
fi

# ── Privacy redaction ────────────────────────────────────────────────────
home_dir=${HOME:-}
host_name=$(cat /proc/sys/kernel/hostname 2>/dev/null || uname -n)
user_name=$(id -un)

# The home path goes first: replacing the user name first would break the
# home path apart (/home/<user>) before it could be matched whole.
redact() {
	local text=$1
	[[ -n $home_dir ]] && text=${text//"$home_dir"/'~'}
	[[ -n $host_name ]] && text=${text//"$host_name"/'<host>'}
	[[ -n $user_name ]] && text=${text//"$user_name"/'<user>'}
	printf '%s' "$text"
}

# Captures a command's output, or "unavailable" when the tool is missing or
# fails; conditions are descriptive, so one missing tool must not stop a run.
probe() {
	local out
	command -v "$1" >/dev/null 2>&1 || {
		printf 'unavailable (%s not installed)' "$1"
		return 0
	}
	if out=$("$@" 2>/dev/null) && [[ -n $out ]]; then
		printf '%s' "$out"
	else
		printf 'unavailable (%s failed)' "$1"
	fi
}

# Enabled outputs as "name WxH@Hz scale S logical LWxLH", one per line, and
# a last line "fits yes|no". Only these fields are printed: kscreen-doctor's
# JSON also carries EDID-derived identifiers that are none of the record's
# business.
display_summary() {
	local json
	command -v kscreen-doctor >/dev/null 2>&1 || {
		echo 'unavailable (kscreen-doctor not installed)'
		echo 'fits unknown'
		return 0
	}
	json=$(kscreen-doctor -j 2>/dev/null) || {
		echo 'unavailable (kscreen-doctor failed)'
		echo 'fits unknown'
		return 0
	}
	/usr/bin/python3 -I -c '
import json, sys
ref_w, ref_h = int(sys.argv[1]), int(sys.argv[2])
try:
    outputs = json.load(sys.stdin)["outputs"]
except (ValueError, KeyError):
    print("unavailable (kscreen-doctor output unreadable)")
    print("fits unknown")
    sys.exit(0)
fits = False
for o in outputs:
    if not o.get("enabled"):
        continue
    mode = next((m for m in o.get("modes", []) if m.get("id") == o.get("currentModeId")), {})
    w, h = mode.get("size", {}).get("width", 0), mode.get("size", {}).get("height", 0)
    # KScreen rotation flags: 2 (left) and 8 (right) swap the axes.
    if o.get("rotation") in (2, 8):
        w, h = h, w
    scale = float(o.get("scale") or 1.0)
    lw, lh = round(w / scale), round(h / scale)
    fits = fits or (lw >= ref_w and lh >= ref_h)
    name, rate = o.get("name"), float(mode.get("refreshRate") or 0)
    print(f"{name} {w}x{h}@{rate:.2f}Hz scale {scale:g} logical {lw}x{lh}")
print("fits " + ("yes" if fits else "no"))
' "$REF_WIDTH" "$REF_HEIGHT" <<<"$json"
}

gpu_summary() {
	if command -v vulkaninfo >/dev/null 2>&1; then
		# deviceUUID and driverUUID are dropped: unique hardware identifiers
		# that add nothing to the conditions.
		vulkaninfo --summary 2>/dev/null |
			sed -n '/^Devices:/,$p' |
			grep -E '^GPU[0-9]+:|deviceType|deviceName|driverName|driverInfo|apiVersion' ||
			echo 'unavailable (vulkaninfo failed)'
	elif command -v glxinfo >/dev/null 2>&1; then
		glxinfo -B 2>/dev/null |
			grep -E 'OpenGL (vendor|renderer|core profile version) string|Device:' ||
			echo 'unavailable (glxinfo failed)'
	else
		echo 'unavailable (neither vulkaninfo nor glxinfo installed)'
	fi
}

power_summary() {
	local supply
	printf 'power profile (powerprofilesctl): %s\n' "$(probe powerprofilesctl get)"
	printf 'tuned profile: %s\n' "$(probe tuned-adm active)"
	printf 'cpufreq governor (cpu0): %s\n' \
		"$(cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor 2>/dev/null || echo unavailable)"
	printf 'energy performance preference (cpu0): %s\n' \
		"$(cat /sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference 2>/dev/null || echo unavailable)"
	for supply in /sys/class/power_supply/*; do
		[[ -r $supply/type && $(cat "$supply/type") == Mains && -r $supply/online ]] || continue
		printf 'mains power online: %s\n' "$(cat "$supply/online")"
	done
}

# The C-14 binding of the measured source. The binary itself cannot prove
# which commit it came from, so the record pairs the commit with the
# binary's SHA-256 and refuses a tree whose build inputs differ from HEAD:
# a run on uncommitted source could never be bound to any commit.
build_inputs=(crates Cargo.lock Cargo.toml rust-toolchain.toml)
if [[ -n $(git -C "$repo_root" status --porcelain -- "${build_inputs[@]}") ]]; then
	die "uncommitted changes under ${build_inputs[*]}; commit or set them aside so the run binds to HEAD"
fi
# Resolved here rather than inside conditions(): a failure inside a command
# substitution does not trip errexit, and a blank binding field must stop
# the run instead of being recorded.
commit=$(git -C "$repo_root" rev-parse HEAD)
crates_tree=$(git -C "$repo_root" rev-parse "$commit":crates)
cargo_lock=$(git -C "$repo_root" rev-parse "$commit":Cargo.lock)
cargo_toml=$(git -C "$repo_root" rev-parse "$commit":Cargo.toml)
rust_toolchain=$(git -C "$repo_root" rev-parse "$commit":rust-toolchain.toml)
bin_sha256=$(sha256sum -- "$bin" | cut -d' ' -f1)
[[ $bin_sha256 =~ ^[0-9a-f]{64}$ ]] || die "could not hash $bin"

conditions() {
	local mem_kib
	mem_kib=$(awk '/^MemTotal:/ { print $2 }' /proc/meminfo)
	echo '# T25 run conditions (written by run.sh; personal identifiers redacted)'
	echo
	echo '## C-14 binding'
	echo "application_commit: $commit"
	echo "crates_tree: $crates_tree"
	echo "cargo_lock: $cargo_lock"
	echo "cargo_toml: $cargo_toml"
	echo "rust_toolchain: $rust_toolchain"
	echo 'build_profile: release (Cargo default profile; no debug sections)'
	echo "binary: ${bin#"$repo_root"/}"
	echo "binary_sha256: $bin_sha256"
	echo
	echo '## Run plan'
	echo "scenarios: ${SCENARIOS[*]}"
	echo "runs_per_scenario: $runs"
	echo "run_length_secs: $secs"
	echo "pause_between_runs_secs: $pause"
	echo 'order: round-robin by run number'
	echo "started_utc: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
	echo
	echo '## Hardware'
	echo "cpu: $(lscpu 2>/dev/null | sed -n 's/^Model name:[[:space:]]*//p' | head -1)"
	echo "logical_cpus: $(nproc)"
	echo "memory: $((mem_kib / 1024 / 1024)) GiB ($mem_kib KiB)"
	echo 'gpu:'
	gpu_summary | sed 's/^[[:space:]]\{1,\}/    /; s/^GPU/  GPU/; s/^OpenGL/  OpenGL/'
	echo
	echo '## Software'
	echo "kernel: $(uname -srm)"
	echo "os: $(cat /etc/fedora-release 2>/dev/null || sed -n 's/^PRETTY_NAME=//p' /etc/os-release)"
	echo "plasma: $(probe plasmashell --version)"
	echo "session_type: ${XDG_SESSION_TYPE:-unset}"
	echo "current_desktop: ${XDG_CURRENT_DESKTOP:-unset}"
	echo "mesa_packages: $(probe rpm -q mesa-vulkan-drivers mesa-dri-drivers | tr '\n' ' ')"
	echo
	echo '## Display'
	display_summary | sed 's/^/  /'
	echo
	echo '## Power'
	power_summary
	echo
	echo '## Load before the first run'
	echo "loadavg: $(cat /proc/loadavg)"
}

conditions_text=$(redact "$(conditions)")
for needle in "$home_dir" "$host_name" "$user_name"; do
	if [[ -n $needle && $conditions_text == *"$needle"* ]]; then
		die "redaction left a personal identifier in the conditions; not recording them"
	fi
done
printf '%s\n' "$conditions_text"

display_fits=$(sed -n 's/^  fits //p' <<<"$conditions_text")
# A dry run only reports the problem: it exists to show the owner what a
# --go would find before any window opens.
if [[ $display_fits == no ]] && $dry_run; then
	echo "run.sh: WARNING: no enabled output's logical area holds ${REF_WIDTH}x${REF_HEIGHT}; --go will refuse" >&2
elif [[ $display_fits == no ]] && ! $allow_small_display; then
	die "no enabled output's logical area holds ${REF_WIDTH}x${REF_HEIGHT}" \
		"(the window would be clamped below the reference client area);" \
		"lower the display scale, or pass --allow-small-display to record the run anyway"
fi

if $dry_run; then
	echo
	echo '## Dry run: would run, in this order'
	for ((n = 1; n <= runs; n++)); do
		for t in "${SCENARIOS[@]}"; do
			echo "FUZZY_DEDUPE_MEASURE=$t,run=$n,secs=$secs ${bin#"$repo_root"/} > raw/$t-run$n.csv"
		done
	done
	exit 0
fi

# ── Measured runs ────────────────────────────────────────────────────────
mkdir -p -- "$raw_dir"
# conditions.txt and runs.tsv are matched by glob too: nullglob drops only
# unmatched patterns, so a literal path would always count as present.
shopt -s nullglob
earlier=("$raw_dir"/*.csv "$raw_dir"/*.partial "$raw_dir"/conditions.tx[t] "$raw_dir"/runs.ts[v])
shopt -u nullglob
if ((${#earlier[@]} > 0)); then
	$replace || die "raw/ already holds an earlier run; move it aside or pass --replace"
	rm -f -- "${earlier[@]}"
fi

printf '%s\n' "$conditions_text" >"$raw_dir/conditions.txt"
if [[ $display_fits == no ]]; then
	echo 'display_override: --allow-small-display (reference client area not available)' \
		>>"$raw_dir/conditions.txt"
fi
printf 'scenario\trun\tsecs\twall_secs\tloadavg_1m_before\n' >"$raw_dir/runs.tsv"

# A stalled app would otherwise hold the session forever: the scripts quit
# by themselves after --secs, so twice that plus startup slack is a hang.
limit=$((secs * 2 + 60))

check_stream() {
	local file=$1 label=$2 n=$3
	[[ -s $file ]] || die "$label run $n: empty sample stream (C-10 fails the run)"
	[[ $(head -n 1 -- "$file") == "$C10_HEADER" ]] ||
		die "$label run $n: stream lacks the C-10 header '$C10_HEADER'"
	# Every sample line names this scenario and run with a numeric value:
	# a stream from the wrong scenario, or a stray stdout line from the app,
	# would otherwise be pooled into the wrong percentiles.
	awk -F, -v s="$label" -v r="$n" '
		NR == 1 { next }
		NF != 4 || $1 != s || $2 != r || $4 !~ /^[0-9]+(\.[0-9]+)?$/ {
			printf "line %d: %s\n", NR, $0 > "/dev/stderr"; bad = 1
		}
		END { if (NR < 2) { print "no sample after the header" > "/dev/stderr"; bad = 1 }
		      exit bad }
	' "$file" || die "$label run $n: malformed sample stream (C-10 fails the run)"
}

for ((n = 1; n <= runs; n++)); do
	for t in "${SCENARIOS[@]}"; do
		label=${t^^}
		out="$raw_dir/$t-run$n.csv"
		load=$(cut -d' ' -f1 /proc/loadavg)
		echo "run.sh: $label run $n of $runs (${secs} s, load $load)" >&2
		start=$SECONDS
		# Standard error stays on the terminal: it carries the app's own
		# diagnostics, which may name local paths and are not evidence.
		# --foreground keeps the app in the terminal's process group, so the
		# owner's Ctrl-C stops the app too instead of only this script.
		if ! FUZZY_DEDUPE_MEASURE="$t,run=$n,secs=$secs" \
			timeout --foreground --kill-after=10 "$limit" "$bin" >"$out.partial"; then
			die "$label run $n: the app exited non-zero or exceeded ${limit} s; partial stream kept as ${out#"$repo_root"/}.partial"
		fi
		wall=$((SECONDS - start))
		check_stream "$out.partial" "$label" "$n"
		((wall >= secs)) ||
			die "$label run $n: finished after ${wall} s, shorter than the configured ${secs} s"
		mv -T -- "$out.partial" "$out"
		printf '%s\t%s\t%s\t%s\t%s\n' "$label" "$n" "$secs" "$wall" "$load" >>"$raw_dir/runs.tsv"
		sleep "$pause"
	done
done

echo "run.sh: all $((runs * ${#SCENARIOS[@]})) runs recorded in ${raw_dir#"$repo_root"/}" >&2
echo "run.sh: next: python3 docs/evidence/ms-1/performance/analyze.py" >&2
