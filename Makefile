#!/usr/bin/env make

build:
	docker build -t performance-service:latest .

run-api:
	docker run \
		--env APP_COMPONENT=api \
		--network=host \
		--env-file=.env \
		-it performance-service:latest

run-api-bg:
	docker run \
		--env APP_COMPONENT=api \
		--network=host \
		--env-file=.env \
		-d performance-service:latest

run-processor:
	docker run \
		--env APP_COMPONENT=processor \
		--network=host \
		--env-file=.env \
		-it performance-service:latest

run-processor-bg:
	docker run \
		--env APP_COMPONENT=processor \
		--network=host \
		--env-file=.env \
		-d performance-service:latest