#!/bin/bash

## This script is used for deb package updates
# COPROC[1] is the stdin for netcat
# COPROC[0] is the stdout of netcat
# By echoing to the stdin of nc, we write to the port 5555
echo "this is deb update" 
#dpkg -i ./*.deb

coproc nc localhost $2

# Send success to uplink
echo "{ \"sequence\": 0, \"timestamp\": $(date +%s%3N), \"action_id\": $1, \"state\": \"Completed\", \"progress\": 100, \"errors\": [] }" >&"${COPROC[1]}"
