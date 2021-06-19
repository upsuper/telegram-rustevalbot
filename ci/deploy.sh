#!/usr/bin/env bash

set -ex

SSH_DIR=$HOME/.ssh
TMP_DIR=/tmp/rustevalbot
DEPLOY_KEY_FILE=$TMP_DIR/deploy_key

mkdir -m0700 "$TMP_DIR"
git rev-parse HEAD > "$TMP_DIR/upgrade"
mkdir -p -m0700 "$SSH_DIR"
cat ci/server_ssh_key >> "$SSH_DIR/known_hosts"
cat <<< "$DEPLOY_KEY" > "$DEPLOY_KEY_FILE"
chmod 0600 "$DEPLOY_KEY_FILE"
sftp -i "$DEPLOY_KEY_FILE" \
     -b ci/deploy.sftp \
     -P 2222 \
     rustevalbot@vps11.upsuper.org
rm -r "$TMP_DIR"
