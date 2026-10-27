#!/bin/bash
pkill labwc; pkill -x lumen ; sleep 0.5
export XDG_RUNTIME_DIR=/tmp/xdgrt; mkdir -p $XDG_RUNTIME_DIR; chmod 700 $XDG_RUNTIME_DIR
export WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1
setsid nohup labwc >/tmp/labwc.log 2>&1 &
sleep 2
ls $XDG_RUNTIME_DIR
