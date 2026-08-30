#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "gym socket verification requires Linux" >&2
  exit 2
fi

socket=${1:-/run/gym/mcp.sock}
group=${2:-gym-access}

[[ -S "$socket" ]] || { echo "missing gym Unix socket: $socket" >&2; exit 1; }
socket_directory=$(dirname -- "$socket")
directory_mode=$(stat -c '%a' "$socket_directory")
directory_group=$(stat -c '%G' "$socket_directory")
[[ "$directory_mode" == "2750" ]] || {
  echo "gym socket directory mode is $directory_mode, expected 2750 (0750 plus setgid)" >&2
  exit 1
}
[[ "$directory_group" == "$group" ]] || {
  echo "gym socket directory group is $directory_group, expected $group" >&2
  exit 1
}
mode=$(stat -c '%a' "$socket")
owner_group=$(stat -c '%G' "$socket")
[[ "$mode" == "660" ]] || { echo "gym socket mode is $mode, expected 660" >&2; exit 1; }
[[ "$owner_group" == "$group" ]] || { echo "gym socket group is $owner_group, expected $group" >&2; exit 1; }

members=$(getent group "$group" | cut -d: -f4)
IFS=',' read -r -a member_array <<<"$members"
for member in "${member_array[@]}"; do
  [[ -z "$member" || "$member" == "boss-agent" ]] || {
    echo "unauthorized explicit $group member: $member" >&2
    exit 1
  }
done

id -nG boss-agent | tr ' ' '\n' | grep -Fxq "$group" || {
  echo "boss-agent is not a member of $group" >&2
  exit 1
}

for forbidden in hermes-gateway research-agent tutor-agent trading-agent; do
  if id "$forbidden" >/dev/null 2>&1 && id -nG "$forbidden" | tr ' ' '\n' | grep -Fxq "$group"; then
    echo "unauthorized service identity in $group: $forbidden" >&2
    exit 1
  fi
done

echo "gym socket boundary valid: $socket mode=660 group=$group peer=boss-agent"
