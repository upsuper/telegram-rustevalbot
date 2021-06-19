mkdir -m0700 /tmp/rustevalbot
git rev-parse HEAD > /tmp/rustevalbot/upgrade
cat server_ssh_key >> $HOME/.ssh/known_hosts
echo "$DEPLOY_KEY" > /tmp/rustevalbot/deploy_key
sftp -i /tmp/rustevalbot/deploy_key \
     -b deploy.sftp \
     -P 2222 \
     rustevalbot@vps11.upsuper.org
rm -r /tmp/rustevalbot
