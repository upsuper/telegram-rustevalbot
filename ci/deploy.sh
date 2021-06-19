#!/usr/bin/env bash

set -ex

mkdir -m0700 /tmp/rustevalbot
git rev-parse HEAD > /tmp/rustevalbot/upgrade
mkdir -p -m0700 $HOME/.ssh
cat ci/server_ssh_key >> $HOME/.ssh/known_hosts
echo "$DEPLOY_KEY" > /tmp/rustevalbot/deploy_key
sftp -i /tmp/rustevalbot/deploy_key \
     -b ci/deploy.sftp \
     -P 2222 \
     rustevalbot@vps11.upsuper.org
rm -r /tmp/rustevalbot
