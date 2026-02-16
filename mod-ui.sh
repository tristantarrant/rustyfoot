#!/bin/sh
DIR=$(dirname $0)
cd $DIR
export MOD_DEV_ENVIRONMENT=0
. modui-env/bin/activate
python server.py

