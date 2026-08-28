#!/usr/bin/env bash
set -euo pipefail

deploy_path=${1:?deployment path is required}
service_name=${2:?systemd service name is required}
release_sha=${3:?release SHA is required}

[[ "$deploy_path" =~ ^/[A-Za-z0-9._/-]+$ ]]
[[ "$service_name" =~ ^[A-Za-z0-9_.@-]+$ ]]
[[ "$release_sha" =~ ^[a-f0-9]{40}$ ]]

archive=/tmp/svetsec-release.tar.gz
release="$deploy_path/releases/$release_sha"
mkdir -p "$release"
tar -xzf "$archive" -C "$release"

if [[ ! -f "$deploy_path/shared/.env" ]]; then
  echo "missing production environment: $deploy_path/shared/.env" >&2
  exit 1
fi
ln -sfn "$deploy_path/shared/.env" "$release/.env"

cd "$release"
npm ci --omit=dev
ln -sfn "$release" "$deploy_path/current"
sudo systemctl restart "$service_name"
rm -f "$archive" /tmp/deploy-release.sh
