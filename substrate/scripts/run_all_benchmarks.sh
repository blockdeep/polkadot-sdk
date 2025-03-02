#!/usr/bin/env bash

# This file is part of Substrate.
# Copyright (C) Parity Technologies (UK) Ltd.
# SPDX-License-Identifier: Apache-2.0
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
# http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

# This script has three parts which all use the Substrate runtime:
# - Pallet benchmarking to update the pallet weights
# - Overhead benchmarking for the Extrinsic and Block weights
# - Machine benchmarking
#
# Should be run on a reference machine to gain accurate benchmarks
# current reference machine: https://github.com/paritytech/substrate/pull/5848

while getopts 'bfp:v' flag; do
  case "${flag}" in
    b)
      # Skip build.
      skip_build='true'
      ;;
    f)
      # Fail if any sub-command in a pipe fails, not just the last one.
      set -o pipefail
      # Fail on undeclared variables.
      set -u
      # Fail if any sub-command fails.
      set -e
      # Fail on traps.
      set -E
      ;;
    p)
      # Start at pallet
      start_pallet="${OPTARG}"
      ;;
    v)
      # Echo all executed commands.
      set -x
      ;;
    *)
      # Exit early.
      echo "Bad options. Check Script."
      exit 1
      ;;
  esac
done


if [ "$skip_build" != true ]
then
  echo "[+] Compiling Substrate benchmarks..."
  cargo build --profile=production --locked --features=runtime-benchmarks --bin substrate-node
fi

# The executable to use.
# Parent directory because of the monorepo structure.
SUBSTRATE=../target/production/substrate-node

# Manually exclude some pallets.
EXCLUDED_PALLETS=(
  # Helper pallets
  "pallet_election_provider_support_benchmarking"
  # Pallets without automatic benchmarking
  "pallet_babe"
  "pallet_grandpa"
  "pallet_mmr"
  "pallet_offences"
  # Only used for testing, does not need real weights.
  "frame_benchmarking_pallet_pov"
)

# Load all pallet names in an array.
ALL_PALLETS=($(
  $SUBSTRATE benchmark pallet --list=pallets --no-csv-header --chain=dev
))

# Filter out the excluded pallets by concatenating the arrays and discarding duplicates.
PALLETS=($({ printf '%s\n' "${ALL_PALLETS[@]}" "${EXCLUDED_PALLETS[@]}"; } | sort | uniq -u))

echo "[+] Benchmarking ${#PALLETS[@]} Substrate pallets by excluding ${#EXCLUDED_PALLETS[@]} from ${#ALL_PALLETS[@]}."

# Define the error file.
ERR_FILE="benchmarking_errors.txt"
# Delete the error file before each run.
rm -f $ERR_FILE

# Benchmark each pallet.
for PALLET in "${PALLETS[@]}"; do
  # If `-p` is used, skip benchmarks until the start pallet.
  if [ ! -z "$start_pallet" ] && [ "$start_pallet" != "$PALLET" ]
  then
    echo "[+] Skipping ${PALLET}..."
    continue
  else
    unset start_pallet
  fi

  FOLDER="$(echo "${PALLET#*_}" | tr '_' '-')";
  WEIGHT_FILE="./frame/${FOLDER}/src/weights.rs"

  TEMPLATE_FILE_NAME="frame-weight-template.hbs"
  if [ $(cargo metadata --locked --format-version 1 --no-deps | jq --arg pallet "${PALLET//_/-}" -r '.packages[] | select(.name == $pallet) | .dependencies | any(.name == "polkadot-sdk-frame")') = true ]
  then
    TEMPLATE_FILE_NAME="frame-umbrella-weight-template.hbs"
  fi
  TEMPLATE_FILE="./.maintain/${TEMPLATE_FILE_NAME}"

  # Special handling of custom weight paths.
  if [ "$PALLET" == "frame_system_extensions" ] || [ "$PALLET" == "frame-system-extensions" ]
  then
    WEIGHT_FILE="./frame/system/src/extensions/weights.rs"
  elif [ "$PALLET" == "pallet_asset_conversion_tx_payment" ] || [ "$PALLET" == "pallet-asset-conversion-tx-payment" ]
  then
    WEIGHT_FILE="./frame/transaction-payment/asset-conversion-tx-payment/src/weights.rs"
  elif [ "$PALLET" == "pallet_asset_tx_payment" ] || [ "$PALLET" == "pallet-asset-tx-payment" ]
  then
    WEIGHT_FILE="./frame/transaction-payment/asset-tx-payment/src/weights.rs"
  elif [ "$PALLET" == "pallet_asset_conversion_ops" ] || [ "$PALLET" == "pallet-asset-conversion-ops" ]
  then
    WEIGHT_FILE="./frame/asset-conversion/ops/src/weights.rs"
  fi

  echo "[+] Benchmarking $PALLET with weight file $WEIGHT_FILE";

  OUTPUT=$(
    $SUBSTRATE benchmark pallet \
    --chain=dev \
    --steps=50 \
    --repeat=20 \
    --pallet="$PALLET" \
    --extrinsic="*" \
    --wasm-execution=compiled \
    --heap-pages=4096 \
    --output="$WEIGHT_FILE" \
    --header="./HEADER-APACHE2" \
    --template="$TEMPLATE_FILE" 2>&1
  )
  if [ $? -ne 0 ]; then
    echo "$OUTPUT" >> "$ERR_FILE"
    echo "[-] Failed to benchmark $PALLET. Error written to $ERR_FILE; continuing..."
  fi
done

# Update the block and extrinsic overhead weights.
echo "[+] Benchmarking block and extrinsic overheads..."
OUTPUT=$(
  $SUBSTRATE benchmark overhead \
  --chain=dev \
  --wasm-execution=compiled \
  --weight-path="./frame/support/src/weights/" \
  --header="./HEADER-APACHE2" \
  --warmup=10 \
  --repeat=100 2>&1
)
if [ $? -ne 0 ]; then
  echo "$OUTPUT" >> "$ERR_FILE"
  echo "[-] Failed to benchmark the block and extrinsic overheads. Error written to $ERR_FILE; continuing..."
fi

echo "[+] Benchmarking the machine..."
OUTPUT=$(
  $SUBSTRATE benchmark machine --chain=dev 2>&1
)
if [ $? -ne 0 ]; then
  # Do not write the error to the error file since it is not a benchmarking error.
  echo "[-] Failed the machine benchmark:\n$OUTPUT"
fi

# Check if the error file exists.
if [ -f "$ERR_FILE" ]; then
  echo "[-] Some benchmarks failed. See: $ERR_FILE"
  exit 1
else
  echo "[+] All benchmarks passed."
  exit 0
fi