#!/usr/bin/env bash
# Install Ubuntu packages without hanging the job. apt's own network
# timeouts are short, each attempt is cut off after a few minutes, and an
# attempt that fails or gets stuck (a slow mirror, a held dpkg lock) is
# tried again.
#
#   .github/scripts/apt-install.sh libgtk-3-dev libudev-dev ...
set -uo pipefail

opts=(
  -o Acquire::Retries=3
  -o Acquire::http::Timeout=30
  -o Acquire::https::Timeout=30
  -o DPkg::Lock::Timeout=120
)

for attempt in 1 2 3; do
  if timeout 300 sudo apt-get "${opts[@]}" update -q &&
     timeout 600 sudo DEBIAN_FRONTEND=noninteractive apt-get "${opts[@]}" install -y -q "$@"; then
    exit 0
  fi
  echo "::warning::apt-get attempt $attempt failed or timed out; trying again"
  sleep 15
done
echo "::error::apt-get failed three times"
exit 1
