#!/bin/bash -xe

mkdir -p ../target/artifacts
cargo build --release
gzip -c ../target/release/pact_verifier_cli > ../target/artifacts/pact_verifier_cli-osx-x86_64.gz
openssl dgst -sha256 -r ../target/artifacts/pact_verifier_cli-osx-x86_64.gz > ../target/artifacts/pact_verifier_cli-osx-x86_64.gz.sha256

# M1
export SDKROOT=$(xcrun -sdk macosx11.1 --show-sdk-path)
export MACOSX_DEPLOYMENT_TARGET=$(xcrun -sdk macosx11.1 --show-sdk-platform-version)
cargo build --target aarch64-apple-darwin --release

gzip -c ../target/aarch64-apple-darwin/release/pact_verifier_cli > ../target/artifacts/pact_verifier_cli-osx-aarch64.gz
openssl dgst -sha256 -r ../target/artifacts/pact_verifier_cli-osx-aarch64.gz > ../target/artifacts/pact_verifier_cli-osx-aarch64.gz.sha256
