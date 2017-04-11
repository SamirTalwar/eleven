.PHONY: test
test: dependencies

.PHONY: dependencies
dependencies:
	@ ./scripts/check-dependencies
