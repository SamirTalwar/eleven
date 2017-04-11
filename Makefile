.PHONY: test
test: dependencies
	[[ -e test/Smoke/bin/smoke ]] || git submodule update --init
	./test/Smoke/bin/smoke test/cases/*

.PHONY: dependencies
dependencies:
	@ ./scripts/check-dependencies
