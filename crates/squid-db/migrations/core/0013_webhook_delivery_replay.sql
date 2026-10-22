-- Store the original request body and target for replaying failed deliveries.

ALTER TABLE core.webhook_deliveries
    ADD COLUMN IF NOT EXISTS target_url TEXT,
    ADD COLUMN IF NOT EXISTS request_body BYTEA,
    ADD COLUMN IF NOT EXISTS signature TEXT;
