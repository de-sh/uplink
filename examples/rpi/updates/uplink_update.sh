#!/bin/bash

## This script updates the agent(uplink)
## Things to do:
##			1. Stop uplink service(systemctl)
##			2. Replace the uplink binary in data partition
## 		3. Restart the uplink service
##			4. Check if uplink can connect to bytebeam cloud

systemctl stop uplink

rm -rf /mnt/next_root/*
tar -xvpzf backup.tar.gz -C /mnt/next_root/
