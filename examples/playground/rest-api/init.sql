-- Playground Tier-3 scenario: REST API polling via WireMock.
-- The init step is a no-op for WireMock — its response mappings live in
-- examples/playground/wiremock/mappings/ which are bind-mounted read-only.

SELECT 'wiremock ready' AS status;
