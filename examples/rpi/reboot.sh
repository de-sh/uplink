<<com
Once update_firmware is triggered,
1. Find the next root, say three (from update_fstab.sh)

2. Create a file, with the same name as next_root in /mnt/download folder
	touch /mnt/download/three
3. Reboot

After reboot,
1. Find the current root (from update_fstab.sh)
2. Compare current root with 
com 
TWO_OK=/boot/two_ok
TWO_BOOT=/boot/two
TWO_DOWNLOAD=/mnt/download/two
THREE_OK=/boot/three_ok
THREE_BOOT=/boot/three
THREE_DOWNLOAD=/mnt/download/three

root_part=`awk -F"root=" '{ print $NF; }' /proc/cmdline | cut -d" " -f1`

if [ ${root_part: -1} = "2" ]
then
	if [ -f "$TWO_BOOT" ]
	then
		rm -rf $TWO_BOOT
	fi
	if [ -f "$TWO_OK" ]
	then
		rm -rf $TWO_OK
	fi
	if [ -f "$TWO_DOWNLOAD" ]
	then
		rm -rf $TWO_DOWNLOAD
	fi
	touch $THREE_DOWNLOAD
	touch $THREE_BOOT
elif [ ${root_part: -1} = "3" ]
then
	if [ -f "$THREE_BOOT" ]
	then
		rm -rf $THREE_BOOT
	fi
	if [ -f "$THREE_OK" ]
	then
		rm -rf $THREE_OK
	fi
	if [ -f "$THREE_DOWNLOAD" ]
	then
		rm -rf $THREE_DOWNLOAD
	touch $TWO_DOWNLOAD
	touch $TWO_BOOT
fi

sudo reboot
