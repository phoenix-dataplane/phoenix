.PHONY: clean compile

WD ?= /tmp/koala

compile:
	cargo b --release && ./scripts/deploy_plugins.sh ${WD}

run: compile
	cargo rr --bin koala

clean:
	cargo clean
	rm -rf ${WD}/build-cache