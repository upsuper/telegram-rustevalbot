git rev-parse HEAD > /tmp/upgrade
cat server_ssh_key >> $HOME/.ssh/known_hosts
openssl aes-256-cbc \
    -K $encrypted_782b57b1cc27_key \
    -iv $encrypted_782b57b1cc27_iv \
    -in deploy_rsa.enc \
    -out /tmp/deploy_rsa \
    -d
chmod 0600 /tmp/deploy_rsa
sftp -i /tmp/deploy_rsa \
     -b deploy.sftp \
     -P 2222 \
     rustevalbot@vps11.upsuper.org
rm /tmp/deploy_rsa
