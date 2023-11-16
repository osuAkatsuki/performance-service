#!/usr/bin/env bash
set -eo pipefail

if [ -z "$APP_ENV" ]; then
  echo "Please set APP_ENV"
  exit 1
fi

if [ -z "$APP_COMPONENT" ]; then
  echo "Please set APP_COMPONENT"
  exit 1
fi

if [[ $PULL_SECRETS_FROM_VAULT -eq 1 ]]; then
  # TODO: revert to $APP_ENV
  akatsuki vault get performance-service production-k8s -o .env
  source .env
fi

# await database availability
/scripts/await-service.sh $DATABASE_HOST $DATABASE_PORT $SERVICE_READINESS_TIMEOUT

# await redis availability
/scripts/await-service.sh $REDIS_HOST $REDIS_PORT $SERVICE_READINESS_TIMEOUT

# await amqp availability
/scripts/await-service.sh $AMQP_HOST $AMQP_PORT $SERVICE_READINESS_TIMEOUT

# run the service (APP_COMPONENT is handled by the service)
exec /usr/local/bin/performance-service
