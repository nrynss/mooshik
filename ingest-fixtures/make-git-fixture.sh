#!/bin/sh
# Creates the git fixture repository for the M8 live run: commit metadata
# only must be ingested — no working-tree files, no patches.
set -eu
cd "$(dirname "$0")"
if [ -d demo-repo ]; then
  rm -rf demo-repo
fi
mkdir demo-repo
git -C demo-repo init -q
git -C demo-repo -c user.name='Fixture Engineer' -c user.email='fixture@example.com' \
  commit -q --allow-empty -m "bootstrap the fixture service" \
  -m "The fixture service is called Cobalt Lantern and it serves weather data."
echo 'AKIAIOSFODNN7EXAMPLE would leak if working trees were walked' > demo-repo/notes.md
git -C demo-repo add notes.md
git -C demo-repo -c user.name='Fixture Engineer' -c user.email='fixture@example.com' \
  commit -q -m "add notes" \
  -m "Body: Cobalt Lantern retries failed fetches three times with jitter."
echo "fixture repo ready: ingest-fixtures/demo-repo (2 commits)"
