-- Audit event types for the membership-billing lifecycle.
--
-- Inserted as rows rather than restated as a CHECK: the permitted set is the
-- union of every row ever added (see
-- 2026-09-02-120000-0000_replace_audit_event_check_with_lookup_table), so a
-- concurrent branch adding its own event type cannot silently drop these on
-- merge. Dated after that migration because the table must exist first.
--
-- These record membership state changes and admin actions; they carry amounts,
-- status, and Stripe reference ids in their payload but never card data.
INSERT INTO audit_event_types (name) VALUES
    ('membership_granted'),
    ('membership_revoked'),
    ('membership_payment_recorded'),
    ('membership_last_admin_protected'),
    ('subscription_started'),
    ('subscription_canceled'),
    ('subscription_payment_failed');
