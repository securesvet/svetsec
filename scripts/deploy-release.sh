#!/usr/bin/env bash
set -euo pipefail

deploy_path=${1:?deployment path is required}
release_sha=${2:?release SHA is required}

[[ "$deploy_path" =~ ^/[A-Za-z0-9._/-]+$ ]]
[[ "$release_sha" =~ ^[a-f0-9]{40}$ ]]

archive=/tmp/svetsec-image.tar
caddy_source=/tmp/Caddyfile
compose_source=/tmp/compose.production.yml
caddy_target="$deploy_path/Caddyfile"
compose_target="$deploy_path/compose.production.yml"
image="svetsec:$release_sha"
image_state="$deploy_path/current-image"

mkdir -p "$deploy_path/shared" "$deploy_path/data"

if [[ ! -f "$deploy_path/shared/.env" ]]; then
  echo "missing production environment: $deploy_path/shared/.env" >&2
  exit 1
fi
if [[ ! -f "$deploy_path/shared/owner_ed25519.pub" ]]; then
  echo "missing SSH owner key: $deploy_path/shared/owner_ed25519.pub" >&2
  exit 1
fi
if [[ ! -f "$deploy_path/shared/ssh_host_ed25519_key" ]]; then
  echo "missing SSH host key: $deploy_path/shared/ssh_host_ed25519_key" >&2
  exit 1
fi
if [[ ! -f "$archive" || ! -f "$caddy_source" || ! -f "$compose_source" ]]; then
  echo "missing Docker deployment files in /tmp" >&2
  exit 1
fi

install -m 0644 "$caddy_source" "$caddy_target"
install -m 0644 "$compose_source" "$compose_target"
docker load --input "$archive"
docker image inspect "$image" >/dev/null

cd "$deploy_path"
SVETSEC_IMAGE="$image" docker compose \
  --env-file shared/.env \
  -f compose.production.yml \
  config --quiet

previous_image=
if [[ -f "$image_state" ]]; then
  previous_image=$(<"$image_state")
fi

if ! SVETSEC_IMAGE="$image" docker compose \
  --env-file shared/.env \
  -f compose.production.yml \
  up -d --remove-orphans --wait --wait-timeout 90
then
  if [[ "$previous_image" =~ ^svetsec:[a-f0-9]{40}$ ]] \
    && docker image inspect "$previous_image" >/dev/null 2>&1
  then
    echo "new container is unhealthy; restoring $previous_image" >&2
    SVETSEC_IMAGE="$previous_image" docker compose \
      --env-file shared/.env \
      -f compose.production.yml \
      up -d --remove-orphans --wait --wait-timeout 90
  fi
  exit 1
fi

printf '%s\n' "$image" > "$image_state.tmp"
mv "$image_state.tmp" "$image_state"

rm -f "$archive" "$caddy_source" "$compose_source" /tmp/deploy-release.sh
