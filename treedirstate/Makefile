NAME := rusttreedirstate

# Cargo's depfiles use full paths, so we must include the full path
# to the rust library.
DIR := $(realpath $(join $(dir $(firstword $(MAKEFILE_LIST))),../rust/treedirstate))

MODE ?= release

ifeq ($(MODE),release)
CARGO_ARGS += --release
endif

ifeq ($(shell uname),Darwin)
LIBSUFFIX := .dylib
else
LIBSUFFIX := .so
endif

RUSTLIB := $(DIR)/target/$(MODE)/lib$(NAME)$(LIBSUFFIX)
RUSTDEP := $(DIR)/target/$(MODE)/lib$(NAME).d

.PHONY: all
all: $(NAME).so

$(NAME).so: $(RUSTLIB)
	cp $< $@

$(RUSTLIB):
	cd $(DIR) && cargo build $(CARGO_ARGS)

.PHONY: test
test:
	cd $(DIR) && cargo test

.PHONY: clean
clean:
	$(RM) $(NAME).so
	cd $(DIR) && cargo clean

# This pattern rule for Rust source files forces a rebuild if any source file
# is deleted.
$(DIR)/src/%.rs: ;

-include $(RUSTDEP)
