# A dummy Makefile that forwards all arguments to cargo-make.
.PHONY: all %

all:
	@cargo make $(ARGS)

%:
	@cargo make $@ $(ARGS)
