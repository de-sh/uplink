root_part=`awk -F"root=" '{ print $NF; }' /proc/cmdline | cut -d" " -f1`
THREE=/home/sai/rpi/three
TWO=/home/sai/rpi/two

if [ -f "$THREE" ]
then
	if [ ${root_part: -1} = "2" ]
	then
		# Boot failed
		rm -rf /boot/three
		# Notify uplink
	elif [ ${root_part: -1} = "3" ]
	then
		# Boot successful
		rm -rf /boot/three_failed
		touch /boot/three_ok
	fi
elif [ -f "$TWO" ]
then
	if [ ${root_part: -1} = "3" ]
	then
		# Boot failed
		rm -rf /boot/two
		# Notify uplink
	elif [ ${root_part: -1} = "2" ]
	then
		# Boot successful
		rm -rf /boot/two_failed
		touch /boot/two_ok
	fi
fi

