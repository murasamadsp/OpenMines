#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
bank="${1:-$root/client/Assets/StreamingAssets/Master.strings.bank}"
sound_manager="$root/client/Assets/Scripts/Utility/SoundManager.cs"
manifest="$root/docs/reference/FMOD_EVENTS.txt"
metadata_dir="$root/client/FMODProject/Metadata"
cache_file="$root/client/Assets/Plugins/FMOD/Cache/Editor/FMODStudioCache.asset"

if [[ ! -f "$bank" ]]; then
  echo "FMOD strings bank not found: $bank" >&2
  exit 2
fi

if [[ ! -f "$manifest" ]]; then
  echo "FMOD event manifest not found: $manifest" >&2
  exit 2
fi

if [[ ! -f "$sound_manager" ]]; then
  echo "SoundManager source not found: $sound_manager" >&2
  exit 2
fi

expected=()
while IFS= read -r path; do
  [[ -z "$path" || "$path" == \#* ]] && continue
  expected+=("$path")
done < "$manifest"

if [[ "${#expected[@]}" -eq 0 ]]; then
  echo "No FMOD event paths found in $manifest" >&2
  exit 2
fi

source_paths="$(mktemp "${TMPDIR:-/tmp}/openmines-fmod-source.XXXXXX")"
grep -Eo '"event:/[^"]+"' "$sound_manager" | tr -d '"' | sort -u >"$source_paths"
trap 'rm -f "$source_paths" "$bank_strings"' EXIT

for path in "${expected[@]}"; do
  if ! grep -Fqx "$path" "$source_paths"; then
    echo "manifest event is not referenced by SoundManager: $path" >&2
    exit 1
  fi
done

missing=0
bank_strings="$(mktemp "${TMPDIR:-/tmp}/openmines-fmod-bank.XXXXXX")"
strings "$bank" >"$bank_strings"

for path in "${expected[@]}"; do
  if ! grep -Fqx "$path" "$bank_strings"; then
    echo "missing FMOD event: $path" >&2
    missing=1
  fi
done

if [[ "$missing" -ne 0 ]]; then
  echo "bank size: $(wc -c <"$bank" | tr -d ' ') bytes" >&2
  if [[ -d "$metadata_dir" ]] || [[ -f "$cache_file" ]]; then
    metadata_hits=0
    for path in "${expected[@]}"; do
      if { [[ -d "$metadata_dir" ]] && grep -FRq -- "$path" "$metadata_dir"; } \
        || { [[ -f "$cache_file" ]] && grep -Fq -- "$path" "$cache_file"; }; then
        metadata_hits=$((metadata_hits + 1))
      fi
    done
    echo "FMOD metadata/cache matching expected event paths: $metadata_hits/${#expected[@]}" >&2
  fi
  echo "FMOD bank is incomplete: $bank" >&2
  exit 1
fi

echo "FMOD bank event contract OK: $bank"
