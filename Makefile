# Read My Receipts — operations Makefile
#
# Targets:
#   Build & Deploy
#     build          Compile release binaries and set port-80 capability
#     deploy         Build + restart (full redeploy)
#
#   Process Management
#     start          Start the kiosk GUI (DISPLAY=:0) on port 80
#     start-headless Start headless server (no GUI) on port 80
#     stop           Stop all receipts processes
#     restart        Stop then start
#     status         Show process status, port binding, USB printer
#
#   Logs & Diagnostics
#     logs           Tail the log file (Ctrl-C to stop)
#     logs-recent    Show last 50 log lines
#     test-print     Send a test photo to the printer
#
#   Camera & Booth
#     preview        Start camera preview on screen
#     preview-stop   Kill camera preview
#     booth          Run booth sequence (3 photos + print strip)
#     snap           Capture a single photo
#
#   Printing
#     print          Print the most recent photo
#     print3         Print the 3 most recent photos as a strip

RELEASE   := target/release
RECEIPTS  := $(RELEASE)/receipts
SERVER    := $(RELEASE)/server
BOOTH     := $(RELEASE)/booth
SNAP      := $(RELEASE)/snap
PRINT_LATEST := $(RELEASE)/print_latest
PICS      := pics
LOG       := /tmp/receipts.log

.PHONY: build deploy start start-headless stop restart status \
        logs logs-recent test-print \
        preview preview-stop booth snap print print3 \
        help

help:
	@echo "Usage: make <target>"
	@echo ""
	@echo "Build & Deploy:"
	@echo "  build          Compile release binaries"
	@echo "  deploy         Build + restart"
	@echo ""
	@echo "Process Management:"
	@echo "  start          Start kiosk GUI on port 80"
	@echo "  start-headless Start headless server on port 80"
	@echo "  stop           Stop all receipts processes"
	@echo "  restart        Stop + start"
	@echo "  status         Process, port, and printer status"
	@echo ""
	@echo "Logs & Diagnostics:"
	@echo "  logs           Tail the log file"
	@echo "  logs-recent    Last 50 log lines"
	@echo "  test-print     Send test photo to printer"
	@echo ""
	@echo "Camera & Booth:"
	@echo "  preview        Start camera preview"
	@echo "  preview-stop   Kill camera preview"
	@echo "  booth          Run booth (3 photos + print)"
	@echo "  snap           Capture single photo"
	@echo ""
	@echo "Printing:"
	@echo "  print          Print most recent photo"
	@echo "  print3         Print 3 most recent as strip"

# ---------------------------------------------------------------------------
# Build & Deploy
# ---------------------------------------------------------------------------

build:
	cargo build --release
	sudo setcap 'cap_net_bind_service=+ep' $(RECEIPTS)
	@test -f $(SERVER) && sudo setcap 'cap_net_bind_service=+ep' $(SERVER) || true
	@test -f $(BOOTH) && sudo setcap 'cap_net_bind_service=+ep' $(BOOTH) || true
	@echo "Build complete."

deploy: build restart

# ---------------------------------------------------------------------------
# Process Management
# ---------------------------------------------------------------------------

start:
	@if pgrep -f 'target/release/receipts' >/dev/null 2>&1; then \
		echo "Already running (PID $$(pgrep -f 'target/release/receipts'))"; \
		exit 1; \
	fi
	DISPLAY=:0 $(RECEIPTS) --kiosk >> $(LOG) 2>&1 &
	@sleep 2
	@if ss -tlnp | grep -q ':80 '; then \
		echo "Started (PID $$(pgrep -f 'target/release/receipts'), port 80)"; \
	else \
		echo "FAILED to bind port 80"; exit 1; \
	fi

start-headless:
	@if pgrep -f 'target/release/server' >/dev/null 2>&1; then \
		echo "Already running (PID $$(pgrep -f 'target/release/server'))"; \
		exit 1; \
	fi
	$(SERVER) >> $(LOG) 2>&1 &
	@sleep 2
	@if ss -tlnp | grep -q ':80 '; then \
		echo "Started (PID $$(pgrep -f 'target/release/server'), port 80)"; \
	else \
		echo "FAILED to bind port 80"; exit 1; \
	fi

stop:
	@pkill -f 'target/release/receipts' 2>/dev/null && echo "Stopped receipts" || true
	@pkill -f 'target/release/server' 2>/dev/null && echo "Stopped server" || true
	@pkill -f 'rpicam-hello' 2>/dev/null && echo "Stopped preview" || true
	@sleep 1
	@echo "All stopped."

restart: stop start

status:
	@echo "=== Process ==="
	@ps aux | grep -E 'target/release/(receipts|server)' | grep -v grep || echo "  Not running"
	@echo ""
	@echo "=== Port 80 ==="
	@ss -tlnp | grep ':80 ' || echo "  Not listening"
	@echo ""
	@echo "=== USB Printer ==="
	@lsusb | grep -i epson || echo "  No Epson printer found"
	@echo ""
	@echo "=== Camera ==="
	@ps aux | grep rpicam | grep -v grep || echo "  No camera process"
	@echo ""
	@echo "=== Display ==="
	@DISPLAY=:0 xset q 2>/dev/null | grep -A1 "DPMS" | tail -1 || echo "  Cannot query display"
	@echo ""
	@echo "=== Last Log Entry ==="
	@tail -3 $(LOG) 2>/dev/null | sed 's/\x1b\[[0-9;]*m//g' || echo "  No log file"

# ---------------------------------------------------------------------------
# Logs & Diagnostics
# ---------------------------------------------------------------------------

logs:
	tail -f $(LOG)

logs-recent:
	@tail -50 $(LOG) | sed 's/\x1b\[[0-9;]*m//g'

test-print:
	@photo=$$(ls -t $(PICS)/*.jpg 2>/dev/null | head -1); \
	if [ -z "$$photo" ]; then echo "No photos in $(PICS)/"; exit 1; fi; \
	echo "Sending $$photo to printer..."; \
	result=$$(curl -s -X POST -F "image=@$$photo" http://localhost:80/print/upload); \
	echo "$$result"

# ---------------------------------------------------------------------------
# Camera & Booth
# ---------------------------------------------------------------------------

preview:
	@pkill -f 'rpicam-hello' 2>/dev/null; sleep 0.5; true
	@sh -c 'DISPLAY=:0 setsid rpicam-hello -t 0 --viewfinder-mode 1332:990:10:P </dev/null >/dev/null 2>&1 & sleep 0.5; pgrep -f rpicam-hello >/dev/null && echo "Preview running (stop with: make preview-stop)" || echo "Failed to start preview"'

preview-stop:
	@pkill -f 'rpicam-hello' 2>/dev/null && echo "Preview stopped" || echo "No preview running"

booth:
	$(BOOTH)

snap:
	$(SNAP)

# ---------------------------------------------------------------------------
# Printing
# ---------------------------------------------------------------------------

print:
	$(PRINT_LATEST)

print3:
	@files=$$(ls -t $(PICS)/*.jpg 2>/dev/null | head -3 | tac); \
	if [ -z "$$files" ]; then echo "No photos in $(PICS)/"; exit 1; fi; \
	count=$$(echo "$$files" | wc -l); i=0; \
	for f in $$files; do \
		i=$$((i + 1)); \
		echo "Printing $$i/$$count: $$f"; \
		curl -s -X POST -F "image=@$$f" http://localhost:80/print/strip; \
		echo; \
		if [ $$i -lt $$count ]; then sleep 2; fi; \
	done
