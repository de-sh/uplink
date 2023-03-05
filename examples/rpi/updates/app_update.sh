#!/bin/bash

## This script updates the user application that is running as a service 
## Things to do:
##			1. Stop the service related to the app
##			2. Replace the application binary in both A, B partitions
## 			3. Restart the application service
##			4. Check if service is running as expected.
##			5. Send the status(success/failure) to uplink

FILE_PATH=.
APP=$1
APP_BIN_PATH=/usr/local/bin
if [ -f /etc/systemd/system/$APP.service ]
then
	# Stop the service
	systemctl stop $APP

	cp $FILE_PATH/$APP $APP_BIN_PATH/

	# Restart the service
	systemctl start $APP

	# Check the status of the service
	# systemctl is-active --quiet $APP.service && exec $APP_STATUS=0
	if [ "$(systemctl is-active --quiet $APP.service)" = "active" ]
	then
		echo "is active"
		# Send status(success) to uplink
		echo "{ \"sequence\": 0, \"timestamp\": $(date +%s%3N), \"action_id\": $2, \"state\": \"Completed\", \"progress\": 100, \"errors\": [] }"

		# Update the other partition also
		cp $FILE_PATH/$APP /mnt/next_root/$APP_BIN_PATH/
	else
		echo "inactive"
		# Send status(failed) to uplink
		echo "{ \"sequence\": 0, \"timestamp\": $(date +%s%3N), \"action_id\": $2, \"state\": \"Failed\", \"progress\": 100, \"errors\": [] }"
	fi
else
	echo "$APP.service does not exist"
fi
