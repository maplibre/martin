#!/bin/bash

cargo build --release
cd ./target/release
zip martin-darwin-x86_64.zip martin
shasum -a 256 martin-darwin-x86_64.zip