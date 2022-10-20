.PHONY: clean compile

WD ?= /tmp/phoenix

compile:
	cargo b --release && ./scripts/deploy_plugins.sh ${WD}

run: compile
	cargo rr --bin phoenix

clean:
	cargo clean
	rm -rf ${WD}/build-cache