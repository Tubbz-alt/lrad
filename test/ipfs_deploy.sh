#!/bin/bash
# Many thanks to Rüdiger for the basis for this script: http://blog.klaehn.org/2018/06/06/publish-blog-on-ipfs/
set -e # halt script on error
# get ipfs (version must match what you have on your node
wget -qO- https://dist.ipfs.io/go-ipfs/v0.4.17/go-ipfs_v0.4.17_linux-amd64.tar.gz | tar xz
PATH=./go-ipfs/:$PATH
# open tunnel to ipfs node
ssh -p $IPFS_SSH_PORT -N -L 5001:localhost:5001 $IPFS_SERVER_ADDRESS &
# wait for some time for the tunnel to be established
sleep 10

if [ "$TRAVIS_BRANCH" = "master" -a "$TRAVIS_PULL_REQUEST" = "false" ]; then
    cargo run -- deploy
else
    echo "Not on master, won't send to git."
    exit 0
fi
