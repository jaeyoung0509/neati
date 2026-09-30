#!/usr/bin/env bash

set -euo pipefail

readonly repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
fixture_root="$(mktemp -d "${TMPDIR:-/tmp}/neati-release-workflow-test.XXXXXX")"
trap 'rm -rf -- "$fixture_root"' EXIT HUP INT TERM

fail() {
  echo "❌ $*" >&2
  exit 1
}

# Check the actual Just recipe without stopping an app or touching /Applications.
recipe="$(cd "$repo_root" && just --dry-run release 2>&1)"
[[ "$recipe" == *"./scripts/tauri_release_build.sh --bundles app"* ]] || fail "release did not request an app-only build"
[[ "$recipe" == *"./scripts/install_release_app.sh"* ]] || fail "release did not invoke the installer"
[[ "$(printf '%s\n' "$recipe" | grep -c '^./scripts/tauri_release_build.sh')" == "1" ]] || fail "release invoked the build more than once"
if printf '%s\n' "$recipe" | grep -qx './scripts/tauri_release_build.sh'; then
  fail "release invoked distribution packaging"
fi
[[ "$recipe" == *"--bundles app"*"./scripts/install_release_app.sh"* ]] || fail "release installed before the build"
[[ "$recipe" == *"node scripts/check_release_dependencies.cjs"*"killall Neati"*"rmSync"* ]] || fail "release did not check dependencies before stopping or cleaning"

mkdir -p "$fixture_root/bin" "$fixture_root/logs"
cat > "$fixture_root/bin/pnpm" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$*" >> "$NEATI_FAKE_PNPM_CALLS"
if [[ "$*" == "--version" ]]; then
  echo "10.fixture"
  exit "${NEATI_FAKE_PNPM_VERSION_STATUS:-0}"
fi
if [[ "$NEATI_FAKE_PNPM_STATUS" == "0" ]]; then
  echo "fixture build succeeded"
else
  echo "fixture build failed" >&2
fi
exit "$NEATI_FAKE_PNPM_STATUS"
EOF
chmod +x "$fixture_root/bin/pnpm"

export NEATI_FAKE_PNPM_CALLS="$fixture_root/calls"
export PATH="$fixture_root/bin:$PATH"
export TMPDIR="$fixture_root/logs"

# Run the real Justfile in a disposable checkout. Every process-control, build
# and install boundary is a fixture; clean-bin may touch only these sentinels.
fixture_repo="$fixture_root/repo"
mkdir -p "$fixture_repo/scripts"
cp "$repo_root/Justfile" "$fixture_repo/Justfile"
cp "$repo_root/scripts/check_release_dependencies.cjs" "$fixture_repo/scripts/"
export NEATI_FAKE_RELEASE_STAGES="$fixture_root/stages"
for command in killall open tauri; do
  cat > "$fixture_root/bin/$command" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$(basename "$0")" >> "$NEATI_FAKE_RELEASE_STAGES"
EOF
  chmod +x "$fixture_root/bin/$command"
done
cat > "$fixture_repo/scripts/tauri_release_build.sh" <<'EOF'
#!/usr/bin/env bash
printf 'build %s\n' "$*" >> "$NEATI_FAKE_RELEASE_STAGES"
EOF
cat > "$fixture_repo/scripts/install_release_app.sh" <<'EOF'
#!/usr/bin/env bash
printf 'install\n' >> "$NEATI_FAKE_RELEASE_STAGES"
EOF
chmod +x "$fixture_repo/scripts/tauri_release_build.sh" "$fixture_repo/scripts/install_release_app.sh"

create_artifacts() {
  mkdir -p "$fixture_repo/dist" "$fixture_repo/target/release/bundle/macos/neati.app"
  printf 'frontend\n' > "$fixture_repo/dist/sentinel"
  printf 'binary\n' > "$fixture_repo/target/release/Neati"
  printf 'bundle\n' > "$fixture_repo/target/release/bundle/macos/neati.app/sentinel"
}

expect_preflight_failure() {
  local recipe_name="$1"
  local reason="$2"
  local status=0
  local output
  create_artifacts
  : > "$NEATI_FAKE_RELEASE_STAGES"
  : > "$NEATI_FAKE_PNPM_CALLS"
  output="$(cd "$fixture_repo" && just "$recipe_name" 2>&1)" || status=$?
  ((status != 0)) || fail "$recipe_name succeeded with unavailable dependencies"
  [[ "$output" == *"$reason"*"pnpm install --frozen-lockfile"* ]] || fail "$recipe_name did not explain the dependency remedy"
  [[ ! -s "$NEATI_FAKE_RELEASE_STAGES" ]] || fail "$recipe_name stopped, built, installed, opened, or used a global CLI after preflight failure"
  [[ ! -s "$NEATI_FAKE_PNPM_CALLS" || "$(cat "$NEATI_FAKE_PNPM_CALLS")" == "--version" ]] || fail "$recipe_name installed packages or started a build after preflight failure"
  [[ "$(cat "$fixture_repo/dist/sentinel")" == "frontend" ]] || fail "$recipe_name removed frontend artifacts"
  [[ "$(cat "$fixture_repo/target/release/Neati")" == "binary" ]] || fail "$recipe_name removed the previous binary"
  [[ "$(cat "$fixture_repo/target/release/bundle/macos/neati.app/sentinel")" == "bundle" ]] || fail "$recipe_name removed the previous app bundle"
}

# A global tauri executable is deliberately present; it must never substitute
# for an absent project dependency, even through the launch wrapper recipes.
for recipe_name in release release-app release-app-and-run release-and-run distribute; do
  expect_preflight_failure "$recipe_name" "the project-local Tauri CLI is missing."
done

mkdir -p "$fixture_repo/node_modules/@tauri-apps/cli"
printf 'process.exit(23);\n' > "$fixture_repo/node_modules/@tauri-apps/cli/tauri.js"
for recipe_name in release distribute; do
  expect_preflight_failure "$recipe_name" "the project-local Tauri CLI could not be started."
done

cat > "$fixture_repo/node_modules/@tauri-apps/cli/tauri.js" <<'EOF'
const { appendFileSync } = require('node:fs');
if (process.argv.slice(2).join(' ') !== '--version') process.exit(24);
appendFileSync(process.env.NEATI_FAKE_RELEASE_STAGES, 'preflight\n');
console.log('tauri-cli 2.fixture');
EOF
export NEATI_FAKE_PNPM_VERSION_STATUS=19
expect_preflight_failure release "pnpm could not be started."
export NEATI_FAKE_PNPM_VERSION_STATUS=0

for recipe_name in release distribute; do
  create_artifacts
  : > "$NEATI_FAKE_RELEASE_STAGES"
  output="$(cd "$fixture_repo" && just "$recipe_name" 2>&1)" || fail "$recipe_name rejected runnable local dependencies"
  if [[ "$recipe_name" == "release" ]]; then
    expected_stages=$'preflight\nkillall\nbuild --bundles app\ninstall'
  else
    expected_stages=$'preflight\nkillall\nbuild '
  fi
  [[ "$(cat "$NEATI_FAKE_RELEASE_STAGES")" == "$expected_stages" ]] || fail "$recipe_name changed the preflight/stop/build/install order"
  [[ ! -e "$fixture_repo/dist/sentinel" && ! -e "$fixture_repo/target/release/Neati" && ! -e "$fixture_repo/target/release/bundle" ]] || fail "$recipe_name skipped cleanup after successful preflight"
done

# Keep the existing real build-wrapper failure-log checks separate from the
# fixture workflow's package-manager availability probes.
: > "$NEATI_FAKE_PNPM_CALLS"
export NEATI_FAKE_PNPM_STATUS=17
build_status=0
output="$(cd "$repo_root" && ./scripts/tauri_release_build.sh --bundles app 2>&1)" || build_status=$?
[[ "$build_status" == "17" ]] || fail "a failed app build returned $build_status instead of 17"
[[ "$output" == *"Tauri build failed (exit 17). Local build log:"* ]] || fail "failure did not name the stage and retained log"
failure_log="${output##*Local build log: }"
[[ -f "$failure_log" ]] || fail "failed build log was removed"
grep -q "fixture build failed" "$failure_log" || fail "failed build output was not retained"
[[ "$(cat "$NEATI_FAKE_PNPM_CALLS")" == "tauri build --bundles app" ]] || fail "unexpected fake build command"

export NEATI_FAKE_PNPM_STATUS=0
success_output="$(cd "$repo_root" && ./scripts/tauri_release_build.sh --bundles app 2>&1)"
[[ "$success_output" == *"fixture build succeeded"* ]] || fail "successful build output was lost"
[[ "$(find "$TMPDIR" -type f -name 'neati-tauri-build.*' | wc -l | tr -d ' ')" == "1" ]] || fail "successful build retained a log"

echo "✅ Release recipe and build-log regression tests passed."
