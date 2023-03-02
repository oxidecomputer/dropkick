#!/usr/bin/env bash
set -euxo pipefail
out=$(dirname -- "${BASH_SOURCE[0]}")/ext4
truncate -s 256M "$out"
# -T default: avoid the "small" usage type
mke2fs -t ext4 -T default -L dropkick-persist -U 7c237b8a-bd81-4708-af04-e00802ecba2b "$out"
zstd -19 --rm --force "$out"
