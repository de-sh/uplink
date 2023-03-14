#!/bin/bash

## This script updates the user application that is running as a service 
## Things to do:
##			1. Stop the service related to the app
##			2. Replace the application binary in both A, B partitions
## 			3. Restart the application service
##			4. Check if service is running as expected.
##			5. Restore prev. app version, if service is inactive
##			6. Send the status(success/failure) to uplink

# COPROC[1] is the stdin for netcat
# COPROC[0] is the stdout of netcat
# By echoing to the stdin of nc, we write to the port 5555

coproc nc localhost 5555

APP=$2
FILE_PATH=$4
if [ "$APP" = "uplink" ]
then
	APP_BIN_PATH=/mnt/download
else
	APP_BIN_PATH=$3
fi

if [ -f /etc/systemd/system/$APP.service ]
then
	# Stop the service
	systemctl stop $APP

	# Rename the application(needed for rollback)
	mv $APP_BIN_PATH/$APP $APP_BIN_PATH/$APP.old
	cp $FILE_PATH/$APP $APP_BIN_PATH/

	# Restart the service
	systemctl start $APP

	# Check the status of the service
	# systemctl is-active --quiet $APP.service && exec $APP_STATUS=0
	if [ "$(systemctl is-active --quiet $APP.service)" = "active" ]
	then
		echo "is active"
		# Send status(success) to uplink)
		echo "{ \"sequence\": 0, \"timestamp\": $(date +%s%3N), \"action_id\": $1, \"state\": \"Completed\", \"progress\": 100, \"errors\": [] }" >& "${COPROC[1]}"

		# Update the other partition also
		if [ "$APP" != "uplink" ]
		then
			cp $FILE_PATH/$APP /mnt/next_root/$APP_BIN_PATH/
		fi
	else
		echo "inactive"
		
		## Rollback feature
		rm $APP_BIN_PATH/$APP
		mv $APP_BIN_PATH/$APP.old $APP_BIN_PATH/$APP
		systemctl start $APP

		# Send status(failed) to uplink)
		echo "{ \"sequence\": 0, \"timestamp\": $(date +%s%3N), \"action_id\": $1, \"state\": \"Failed\", \"progress\": 100, \"errors\": [] }" >& "${COPROC[1]}"
	fi
else
	echo "$APP.service does not exist"
fi
