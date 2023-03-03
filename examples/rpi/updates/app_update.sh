#!/bin/bash

## This script updates the user application that is running as a service 
## Things to do:
##			1. Stop the service related to the app
##			2. Replace the application binary in both A, B partitions
## 		3. Restart the application service
##			4. Check if service is running as expected.
##			5. Send the status(success/failure) to uplink

app=$1
systemctl stop $app
sudo cp /etc/systemd/system/uplink.service /etc/systemd/systemd/uplink.service.old
