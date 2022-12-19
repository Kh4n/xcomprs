Xephyr -br -ac -screen 800x600 :1 &
(sleep 1 && (./target/debug/xcomprs &) DISPLAY:=1 openbox)