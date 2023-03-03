#!/bin/bash
## To be placed in /tmp folder

if [ ! -f /tmp/setup_done ]
then
	curl  --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/bytebeamio/sai-kiran-y/main/examples/rpi/one_time_setup.sh | bash 
	touch /tmp/setup_done
fi
