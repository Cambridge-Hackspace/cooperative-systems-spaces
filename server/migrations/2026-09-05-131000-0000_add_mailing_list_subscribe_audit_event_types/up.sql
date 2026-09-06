-- Two event types for platform-driven mailing-list opt-in and opt-out.
--
-- Inserted as rows rather than restated as a CHECK: the permitted set is the
-- union of every row ever added (see
-- 2026-09-02-120000-0000_replace_audit_event_check_with_lookup_table), so a
-- concurrent branch adding its own event type cannot silently drop this one on
-- merge. Dated after that migration because the table must exist first.
INSERT INTO audit_event_types (name) VALUES
    ('mailing_list_subscribe'),
    ('mailing_list_unsubscribe');
