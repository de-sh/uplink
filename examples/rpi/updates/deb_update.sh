#!/bin/bash

## This script is used for deb package updates

dpkg -i ./*.deb

# Send success to uplink
echo "{ \"sequence\": 0, \"timestamp\": $(date +%s%3N), \"action_id\": $1, \"state\": \"Completed\", \"progress\": 100, \"errors\": [] }"

