#!/bin/bash

curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
rustup update
sudo apt install musl musl-dev musl-tools

git clone https://github.com/slundi/RatioUp.git
cd RatioUp
git pull
cargo build --release
ln -s target/release/RatioUp

echo "@reboot cd $(pwd) && ./RatioUp" | crontab -
# rustup self uninstall
