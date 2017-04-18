.PHONY: test
test: dependencies
	@ ./test/run

.PHONY: dependencies
dependencies:
	@ ./scripts/check-dependencies
