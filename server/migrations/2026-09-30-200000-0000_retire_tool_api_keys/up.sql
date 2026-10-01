-- Retire the alternate tool credentials (#101 slice 6).
--
-- ToolGuard accepted three credentials: a registered device's Bearer token, a
-- per-tool `tools.external_api_key`, and one shared `toolguard.global_api_key`.
-- The weakest of the set decided the real bar, and the weakest was a single
-- secret that opened every tool. Authentication is now the device token alone,
-- scoped by the explicit `device_bindings` row an administrator made -- which is
-- the association the rest of #101 made uniform.
--
-- The per-tool key also served as the metered-billing binding: a billable report
-- had to carry the tool's own secret so one leaked shared key could not post
-- charges everywhere. That property is preserved and strengthened rather than
-- dropped -- a metered report now requires a device bound to that tool in the
-- `power` role, which is equally per-tool and whose secret is SHA-256 hashed at
-- rest (#120/#14) where this column held plaintext.
--
-- The global key lived only in configuration, so it needs no migration; this is
-- the per-tool half.

DROP INDEX IF EXISTS idx_tools_external_api_key;
ALTER TABLE tools DROP COLUMN external_api_key;
