#!/bin/sh
DIR=$(dirname $0)
cd $DIR
export MOD_DEV_ENVIRONMENT=0
export MOD_HMI_TRANSPORT=tcp
export MOD_HMI_TCP_PORT=9898
. modui-env/bin/activate
python server.py

