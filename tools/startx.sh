#!/bin/bash
# start Xvfb + dbus session + xfwm4 detached
pkill -x Xvfb ; pkill xfwm4; pkill xfconfd; sleep 0.5
setsid nohup Xvfb :7 -screen 0 1280x720x24 -ac >/tmp/xvfb.log 2>&1 &
sleep 1
export DISPLAY=:7
eval $(dbus-launch --sh-syntax)
echo "export DBUS_SESSION_BUS_ADDRESS='$DBUS_SESSION_BUS_ADDRESS'" > /tmp/dbusenv
setsid nohup xfwm4 --compositor=${1:-off} >/tmp/xfwm.log 2>&1 &
sleep 2
xprop -root _NET_SUPPORTING_WM_CHECK
