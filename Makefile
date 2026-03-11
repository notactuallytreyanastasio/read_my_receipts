RELEASE := target/release
SERVER := $(RELEASE)/server
RECEIPTS := $(RELEASE)/receipts
SNAP := $(RELEASE)/snap
PRINT_LATEST := $(RELEASE)/print_latest
PICS := pics

.PHONY: build serve serve-gui logs snap print print3 preview preview-stop booth

build:
	cargo build --release
	sudo setcap 'cap_net_bind_service=+ep' $(SERVER)
	sudo setcap 'cap_net_bind_service=+ep' $(RECEIPTS)

serve: build
	@pkill -f 'target/release/server' 2>/dev/null; pkill -f 'receipts --kiosk' 2>/dev/null; sleep 1
	$(SERVER) >> /tmp/receipts.log 2>&1 &
	@sleep 2 && ss -tlnp | grep -q ':80 ' && echo "Running on port 80" || echo "FAILED to bind port 80"

serve-gui: build
	@pkill -f 'receipts --kiosk' 2>/dev/null; pkill -f 'target/release/server' 2>/dev/null; sleep 1
	DISPLAY=:0 $(RECEIPTS) --kiosk >> /tmp/receipts.log 2>&1 &
	@sleep 2 && ss -tlnp | grep -q ':80 ' && echo "Running on port 80" || echo "FAILED to bind port 80"

logs:
	tail -f /tmp/receipts.log

snap:
	$(SNAP)

print:
	$(PRINT_LATEST)

print3:
	@files=$$(ls -t $(PICS)/*.jpg 2>/dev/null | head -3 | tac); \
	if [ -z "$$files" ]; then echo "No photos in $(PICS)/"; exit 1; fi; \
	count=$$(echo "$$files" | wc -l); i=0; \
	for f in $$files; do \
		i=$$((i + 1)); \
		echo "Printing $$i/$$count: $$f"; \
		curl -s -X POST -F "image=@$$f" http://localhost:80/print/upload; \
		echo; \
		if [ $$i -lt $$count ]; then sleep 2; fi; \
	done

preview:
	@pkill -f 'rpicam-hello' 2>/dev/null; sleep 0.5; true
	@sh -c 'DISPLAY=:0 setsid rpicam-hello -t 0 --viewfinder-mode 1332:990:10:P </dev/null >/dev/null 2>&1 & sleep 0.5; pgrep -f rpicam-hello >/dev/null && echo "Preview running (stop with: make preview-stop)" || echo "Failed to start preview"'

preview-stop:
	@pkill -f 'rpicam-hello' 2>/dev/null && echo "Preview stopped" || echo "No preview running"

booth:
	$(RELEASE)/booth
