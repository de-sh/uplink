#!/bin/bash

## Wrapper script to select appropriate update_script

"""
## App update without rollback
FILE_PATH=.
APP_NAME=my_app
APP_BIN_PATH=/usr/local/bin
./app_update.sh $1 $APP_NAME $APP_BIN_PATH $FILE_PATH
"""

"""
## App update with rollback
FILE_PATH=.
APP_NAME=my_app
APP_BIN_PATH=/usr/local/bin
./app_update_rollback.sh $1 $APP_NAME $APP_BIN_PATH $FILE_PATH
"""

"""
## Deb update
./deb_update.sh $1
"""

"""
## rootfs update
./rootfs_update.sh $1
"""
