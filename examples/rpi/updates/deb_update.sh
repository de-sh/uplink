#!/bin/bash

## This script is used for deb package updates

dpkg -i ./*.deb

# Send success to uplink
