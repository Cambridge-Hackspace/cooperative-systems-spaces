-- #179: start sequencing and run-on for power bindings, and the in-use rule.
--
-- A tool may have any number of `power` bindings. Until now the edge leased
-- them all at once and for as long as the interlocks allowed -- which also
-- meant a standalone plug was energized whether or not anyone was signed on.
-- Three per-binding timings let several plugs come on in stages, each only
-- after the stage before it has CONFIRMED it energized, and go off with a
-- run-on for the ones that need it. All default to 0, so every existing
-- binding behaves as before: first stage, no delay, no run-on.

ALTER TABLE device_bindings
    ADD COLUMN start_stage INTEGER NOT NULL DEFAULT 0
        CONSTRAINT device_bindings_start_stage_nonneg CHECK (start_stage >= 0),
    ADD COLUMN start_delay_ms INTEGER NOT NULL DEFAULT 0
        CONSTRAINT device_bindings_start_delay_nonneg CHECK (start_delay_ms >= 0),
    ADD COLUMN run_on_ms INTEGER NOT NULL DEFAULT 0
        CONSTRAINT device_bindings_run_on_nonneg CHECK (run_on_ms >= 0);

COMMENT ON COLUMN device_bindings.start_stage IS
    'Power bindings in a lower stage must confirm they are energized (a power report with relay_on true, since the tool was allowed) before this one is leased. Any number of bindings per stage.';
COMMENT ON COLUMN device_bindings.start_delay_ms IS
    'After the stage before it confirms -- or, for the first stage, after the tool is allowed -- wait this long before leasing: spin-up time.';
COMMENT ON COLUMN device_bindings.run_on_ms IS
    'After any denial, keep this binding leased this long. Applies to the coordinator''s decisions, never to its silence.';
